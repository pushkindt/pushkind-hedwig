use std::collections::HashSet;
use std::convert::TryFrom;

use async_imap::Session;
use pushkind_common::zmq::{ZmqSender, ZmqSenderExt};
use pushkind_emailer::domain::email::{EmailRecipient, UpdateEmailRecipient};
use pushkind_emailer::domain::hub::Hub;
use pushkind_emailer::domain::types::{EmailRecipientId, EmailRecipientReply, HubId, ImapUid};
use pushkind_emailer::models::zmq::{ZMQReplyMessage, ZMQUnsubscribeMessage};
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep};
use tokio_rustls::client::TlsStream;

use crate::errors::Error;
use crate::repository::{DieselRepository, EmailReader, EmailWriter, HubWriter};

use super::imap::{fetch_message_rfc822, init_session};
use super::parser::parse_email;

async fn send_unsubscribe_message(
    repo: &(impl EmailWriter + ?Sized),
    zmq_sender: &ZmqSender,
    hub_id: HubId,
    email: String,
    reason: Option<String>,
) {
    match repo.unsubscribe_recipient(&email, hub_id, reason.as_deref()) {
        Ok(_) => log::info!("Persisted unsubscribe for {email} in hub#{hub_id}"),
        Err(err) => {
            log::error!("Cannot persist unsubscribe for {email} in hub#{hub_id}: {err}");
        }
    }

    let message = ZMQUnsubscribeMessage {
        hub_id: hub_id.get(),
        email: email.clone(),
        reason,
    };

    match zmq_sender.send_json(&message).await {
        Ok(_) => log::info!("ZMQ unsubscribe message sent for {email} in hub#{hub_id}"),
        Err(err) => {
            log::error!("Cannot send ZMQ unsubscribe message for {email} in hub#{hub_id}: {err}")
        }
    }
}

async fn send_reply_message(
    zmq_sender: &ZmqSender,
    hub_id: HubId,
    email: &str,
    reply: Option<&str>,
    subject: Option<&str>,
) {
    let message = ZMQReplyMessage {
        hub_id: hub_id.get(),
        email: email.to_owned(),
        message: reply.unwrap_or_default().to_string(),
        subject: subject.map(str::to_string),
    };

    match zmq_sender.send_json(&message).await {
        Ok(_) => {
            log::info!("ZMQ message sent for {email} in hub#{hub_id}");
        }
        Err(e) => {
            log::error!("Cannot send ZMQ message for {email} in hub#{hub_id}: {e}");
        }
    }
}

pub async fn process_reply(
    repo: &(impl EmailWriter + ?Sized),
    recipient: &EmailRecipient,
    reply: Option<String>,
) {
    let reply = reply.and_then(|reply| match EmailRecipientReply::try_from(reply) {
        Ok(reply) => Some(reply),
        Err(err) => {
            log::warn!(
                "Skipping invalid reply for recipient {}: {}",
                recipient.id,
                err
            );
            None
        }
    });

    if let Err(e) = repo.update_recipient(
        recipient.id,
        &UpdateEmailRecipient {
            is_sent: Some(true),
            replied: Some(true),
            opened: Some(true),
            reply,
        },
    ) {
        log::error!("Cannot set email recipient replied status: {e}");
    } else {
        log::info!("Email recipient replied status set for {}", recipient.id);
    }
}

pub async fn process_new_message(
    repo: &(impl EmailReader + EmailWriter + ?Sized),
    session: &mut Session<TlsStream<TcpStream>>,
    uid: u32,
    domain: &str,
    hub_id: HubId,
    zmq_sender: &ZmqSender,
) {
    let raw_message = match fetch_message_rfc822(session, uid).await {
        Some(raw) => raw,
        None => return,
    };

    let parsed = match parse_email(&raw_message, domain) {
        Ok(parsed) => parsed,
        Err(err) => {
            log::error!("Cannot parse email UID {} in hub#{}: {}", uid, hub_id, err);
            return;
        }
    };

    if let Some(subject) = parsed.subject.as_ref() {
        if subject.eq_ignore_ascii_case("unsubscribe") {
            match parsed.sender_email.clone() {
                Some(email) => {
                    send_unsubscribe_message(
                        repo,
                        zmq_sender,
                        hub_id,
                        email,
                        Some(subject.clone()),
                    )
                    .await;
                    return;
                }
                None => log::warn!(
                    "Received unsubscribe email without sender in hub#{}",
                    hub_id
                ),
            }
        } else if subject.eq_ignore_ascii_case("Undelivered Mail Returned to Sender") {
            if let Some(email) = parsed.bounce_recipient.clone() {
                send_unsubscribe_message(repo, zmq_sender, hub_id, email, Some(subject.clone()))
                    .await;
                return;
            } else {
                log::warn!(
                    "Undelivered email without identifiable recipient in hub#{}",
                    hub_id
                );
            }
        }
    }

    if let Some(recipient_id) = parsed.recipient_id {
        let reply = parsed.reply.clone();
        let recipient_id = match EmailRecipientId::try_from(recipient_id) {
            Ok(id) => id,
            Err(err) => {
                log::warn!(
                    "Skipping recipient id {} in hub#{}: {}",
                    recipient_id,
                    hub_id,
                    err
                );
                return;
            }
        };

        match repo.get_email_recipient_by_id(recipient_id, hub_id) {
            Ok(Some(recipient)) => {
                process_reply(repo, &recipient, reply).await;
            }
            Ok(None) => log::warn!(
                "Recipient not found for id {} in hub#{}",
                recipient_id.get(),
                hub_id,
            ),
            Err(e) => log::error!(
                "Failed to load recipient id {} in hub#{}: {}",
                recipient_id.get(),
                hub_id,
                e,
            ),
        }
    }

    let reply = parsed.reply.as_deref();
    let subject = parsed.subject.as_deref();
    if let Some(email) = parsed.sender_email.as_deref() {
        send_reply_message(zmq_sender, hub_id, email, reply, subject).await;
    } else {
        log::warn!(
            "Cannot send ZMQ reply message in hub#{}: missing sender email",
            hub_id
        );
    }
}

fn persist_last_processed_uid(
    repo: &(impl HubWriter + ?Sized),
    hub_id: HubId,
    stored_uid: &mut ImapUid,
    candidate_uid: u32,
) {
    let Ok(new_uid) = i32::try_from(candidate_uid) else {
        log::warn!(
            "Skipping IMAP UID persistence for hub#{} because {} exceeds i32 bounds",
            hub_id,
            candidate_uid
        );
        return;
    };

    let new_uid = match ImapUid::try_from(new_uid) {
        Ok(uid) => uid,
        Err(err) => {
            log::warn!(
                "Skipping IMAP UID persistence for hub#{} because {} is invalid: {}",
                hub_id,
                candidate_uid,
                err
            );
            return;
        }
    };

    if new_uid.get() <= stored_uid.get() {
        return;
    }

    match repo.set_imap_last_uid(hub_id, new_uid) {
        Ok(_) => {
            *stored_uid = new_uid;
            log::debug!(
                "Persisted IMAP last UID {} for hub#{}",
                stored_uid.get(),
                hub_id
            );
        }
        Err(err) => log::error!(
            "Cannot persist IMAP last UID {} for hub#{}: {}",
            new_uid.get(),
            hub_id,
            err
        ),
    }
}

fn ordered_uids(uids: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut ordered: Vec<u32> = uids.into_iter().collect();
    ordered.sort_unstable();
    ordered
}

pub async fn monitor_hub(
    repo: DieselRepository,
    hub: Hub,
    domain: String,
    zmq_sender: &ZmqSender,
) -> Result<(), Error> {
    let (imap_server, imap_port, username, password) =
        match (&hub.imap_server, hub.imap_port, &hub.login, &hub.password) {
            (Some(server), Some(port), Some(username), Some(password)) => (
                server.as_str(),
                port.get(),
                username.as_str(),
                password.as_str(),
            ),
            _ => {
                return Err(Error::Config(format!(
                    "Cannot get imap server and port for the hub#{}",
                    hub.id
                )));
            }
        };

    let mut session = init_session(imap_server, imap_port, username, password).await?;

    let mut last_uid: u32 = hub.imap_last_uid.get() as u32;
    let mut persisted_uid = hub.imap_last_uid;

    let initial_search = format!("UID {}:*", last_uid.saturating_add(1));
    let initial_uids = match session.uid_search(&initial_search).await {
        Ok(uids) => uids,
        Err(e) => {
            log::error!("Cannot fetch initial IMAP backlog in hub#{}: {e}", hub.id);
            HashSet::<u32>::new()
        }
    };

    let cutoff_uid = last_uid;
    for uid in ordered_uids(initial_uids.into_iter())
        .into_iter()
        .filter(|&uid| uid != cutoff_uid)
    {
        process_new_message(&repo, &mut session, uid, &domain, hub.id, zmq_sender).await;
        last_uid = uid;
        persist_last_processed_uid(&repo, hub.id, &mut persisted_uid, uid);
    }

    log::info!("Starting a monitoring loop for hub#{}", hub.id);
    loop {
        let mut idle = session.idle();
        if let Err(e) = idle.init().await {
            log::error!("Idle start error in hub#{}: {e}", hub.id);
            let _ = idle.done().await; // attempt to recover
            return Err(e.into());
        }
        let (wait, stop) = idle.wait();
        let keepalive = tokio::spawn(async move {
            sleep(Duration::from_secs(60 * 29)).await;
            drop(stop);
        });

        if let Err(e) = wait.await {
            if let async_imap::error::Error::Io(ref io_err) = e {
                if io_err.kind() == std::io::ErrorKind::TimedOut {
                    // keepalive triggered; not a fatal error
                } else {
                    log::error!("Idle error in hub#{}: {e}", hub.id);
                    let _ = idle.done().await;
                    return Err(e.into());
                }
            } else {
                log::error!("Idle error in hub#{}: {e}", hub.id);
                let _ = idle.done().await;
                return Err(e.into());
            }
        }

        keepalive.abort();
        let _ = keepalive.await;
        session = match idle.done().await {
            Ok(s) => s,
            Err(e) => {
                log::error!("Idle done error in hub#{}: {e}", hub.id);
                return Err(e.into());
            }
        };

        let search_query = format!("UID {}:*", last_uid.saturating_add(1));
        let new_uids = match session.uid_search(&search_query).await {
            Ok(uids) => uids,
            Err(e) => {
                log::error!("Cannot search new emails in hub#{}: {e}", hub.id);
                continue;
            }
        };

        for uid in ordered_uids(new_uids.into_iter()) {
            process_new_message(&repo, &mut session, uid, &domain, hub.id, zmq_sender).await;
            last_uid = uid;
            persist_last_processed_uid(&repo, hub.id, &mut persisted_uid, uid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pushkind_common::repository::errors::RepositoryResult;
    use pushkind_emailer::domain::types::{HubId, ImapUid};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingHubWriter {
        calls: Arc<Mutex<Vec<(HubId, ImapUid)>>>,
    }

    impl HubWriter for RecordingHubWriter {
        fn set_imap_last_uid(&self, hub_id: HubId, uid: ImapUid) -> RepositoryResult<()> {
            self.calls
                .lock()
                .expect("lock poisoned")
                .push((hub_id, uid));
            Ok(())
        }
    }

    fn persist_uids_in_order(
        repo: &(impl HubWriter + ?Sized),
        hub_id: HubId,
        last_uid: &mut u32,
        persisted_uid: &mut ImapUid,
        uids: impl IntoIterator<Item = u32>,
    ) {
        for uid in ordered_uids(uids) {
            *last_uid = uid;
            persist_last_processed_uid(repo, hub_id, persisted_uid, uid);
        }
    }

    #[test]
    fn advances_and_persists_single_uid() {
        let repo = RecordingHubWriter::default();
        let mut last_uid = 0_u32;
        let mut persisted_uid = ImapUid::try_from(0).unwrap();

        persist_uids_in_order(
            &repo,
            HubId::try_from(7).unwrap(),
            &mut last_uid,
            &mut persisted_uid,
            [42_u32],
        );

        assert_eq!(last_uid, 42);
        assert_eq!(persisted_uid.get(), 42);
        assert_eq!(
            repo.calls.lock().expect("lock poisoned").as_slice(),
            &[(HubId::try_from(7).unwrap(), ImapUid::try_from(42).unwrap())]
        );
    }

    #[test]
    fn processes_in_uid_order_and_persists_each_step() {
        let repo = RecordingHubWriter::default();
        let mut last_uid = 0_u32;
        let mut persisted_uid = ImapUid::try_from(0).unwrap();

        persist_uids_in_order(
            &repo,
            HubId::try_from(1).unwrap(),
            &mut last_uid,
            &mut persisted_uid,
            [5_u32, 3_u32, 4_u32],
        );
        assert_eq!(last_uid, 5);
        assert_eq!(persisted_uid.get(), 5);
        assert_eq!(
            repo.calls.lock().expect("lock poisoned").as_slice(),
            &[
                (HubId::try_from(1).unwrap(), ImapUid::try_from(3).unwrap()),
                (HubId::try_from(1).unwrap(), ImapUid::try_from(4).unwrap()),
                (HubId::try_from(1).unwrap(), ImapUid::try_from(5).unwrap())
            ]
        );
    }
}
