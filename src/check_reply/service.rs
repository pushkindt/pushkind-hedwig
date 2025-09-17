use std::str;

use async_imap::Session;
use futures::StreamExt;
use once_cell::sync::Lazy;
use pushkind_common::domain::emailer::email::{EmailRecipient, UpdateEmailRecipient};
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::{ZMQReplyMessage, ZMQUnsubscribeMessage};
use pushkind_common::zmq::ZmqSender;
use regex::Regex;
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep};
use tokio_rustls::client::TlsStream;

use crate::errors::Error;
use crate::repository::{DieselRepository, EmailReader, EmailWriter};

use super::imap::{fetch_message_body, init_session};
use super::parser::{extract_plain_reply, extract_recipient_id};

static EMAIL_REGEX: Lazy<Option<Regex>> =
    Lazy::new(|| Regex::new(r"(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}").ok());

fn extract_header_value(header: &str, name: &str) -> Option<String> {
    let mut collected = String::new();
    let mut found = false;
    let header_name = format!("{}:", name.to_ascii_lowercase());

    for raw_line in header.lines() {
        let line = raw_line.trim_end_matches('\r');
        if found {
            if line.starts_with(' ') || line.starts_with('\t') {
                if !collected.is_empty() {
                    collected.push(' ');
                }
                collected.push_str(line.trim());
                continue;
            }
            break;
        }

        let lower_line = line.to_ascii_lowercase();
        if lower_line.starts_with(&header_name)
            && let Some((_, value)) = line.split_once(':')
        {
            collected.push_str(value.trim());
            found = true;
        }
    }

    if found { Some(collected) } else { None }
}

fn extract_email_address(input: &str) -> Option<String> {
    match &*EMAIL_REGEX {
        Some(regex) => regex.find(input).map(|m| m.as_str().to_string()),
        None => {
            log::error!("Email regex failed to compile");
            None
        }
    }
}

fn extract_sender_email(header: &str) -> Option<String> {
    for field in ["Sender", "From"] {
        if let Some(value) = extract_header_value(header, field)
            && let Some(email) = extract_email_address(&value)
        {
            return Some(email);
        }
    }
    None
}

fn extract_bounce_recipient(body: &str) -> Option<String> {
    let mut fallback = None;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(email) = extract_email_address(line) {
            let lower = line.to_ascii_lowercase();
            if lower.contains("final-recipient")
                || lower.contains("original-recipient")
                || lower.contains("for <")
                || lower.contains("for ")
                || lower.contains("recipient:")
            {
                return Some(email);
            }

            if fallback.is_none() && !lower.contains("mailer-daemon") {
                fallback = Some(email);
            }
        }
    }

    fallback
}

async fn send_unsubscribe_message(
    zmq_sender: &ZmqSender,
    hub_id: i32,
    email: String,
    reason: Option<String>,
) {
    let message = ZMQUnsubscribeMessage {
        hub_id,
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

pub async fn process_reply(
    repo: &DieselRepository,
    hub_id: i32,
    recipient: &EmailRecipient,
    reply: Option<String>,
    zmq_sender: &ZmqSender,
) {
    let msg = ZMQReplyMessage {
        hub_id,
        email: recipient.address.clone(),
        message: reply.clone().unwrap_or_default(),
    };
    if let Err(e) = zmq_sender.send_json(&msg).await {
        log::error!("Cannot send ZMQ message: {e}");
    } else {
        log::info!("ZMQ message sent for email id: {}", recipient.email_id);
    }
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
    repo: &DieselRepository,
    session: &mut Session<TlsStream<TcpStream>>,
    uid: u32,
    domain: &str,
    hub_id: i32,
    zmq_sender: &ZmqSender,
) {
    let mut fetches = match session.uid_fetch(uid.to_string(), "RFC822.HEADER").await {
        Ok(f) => f,
        Err(e) => {
            log::error!("Cannot fetch header for UID {uid}: {e}");
            return;
        }
    };

    let fetch = match fetches.next().await {
        Some(Ok(f)) => f,
        Some(Err(e)) => {
            log::error!("Cannot fetch header for UID {uid}: {e}");
            return;
        }
        None => return,
    };
    drop(fetches);

    let header = match fetch.header() {
        Some(h) => h,
        None => return,
    };

    let header_str = match str::from_utf8(header) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Cannot parse header utf8: {e}");
            return;
        }
    };

    if let Some(subject) = extract_header_value(header_str, "Subject") {
        if subject.eq_ignore_ascii_case("unsubscribe") {
            match extract_sender_email(header_str) {
                Some(email) => {
                    send_unsubscribe_message(zmq_sender, hub_id, email, Some(subject.clone()))
                        .await;
                    return;
                }
                None => log::warn!(
                    "Received unsubscribe email without sender in hub#{}",
                    hub_id
                ),
            }
        } else if subject.eq_ignore_ascii_case("Undelivered Mail Returned to Sender") {
            match fetch_message_body(session, uid).await {
                Some(body) => match extract_bounce_recipient(&body) {
                    Some(email) => {
                        send_unsubscribe_message(zmq_sender, hub_id, email, Some(subject.clone()))
                            .await;
                        return;
                    }
                    None => log::warn!(
                        "Undelivered email without identifiable recipient in hub#{}",
                        hub_id
                    ),
                },
                None => log::warn!("Cannot fetch body for undelivered email in hub#{}", hub_id),
            }
        }
    }

    if let Some(recipient_id) = extract_recipient_id(header_str, domain) {
        let reply = fetch_message_body(session, uid)
            .await
            .map(|b| extract_plain_reply(&b));
        match repo.get_email_recipient_by_id(recipient_id, hub_id) {
            Ok(Some(recipient)) => process_reply(repo, hub_id, &recipient, reply, zmq_sender).await,
            Ok(None) => log::warn!(
                "Recipient not found for id {} in hub#{}",
                recipient_id,
                hub_id,
            ),
            Err(e) => log::error!(
                "Failed to load recipient id {} in hub#{}: {}",
                recipient_id,
                hub_id,
                e,
            ),
        }
    }
}

pub async fn monitor_hub(
    repo: DieselRepository,
    hub: Hub,
    domain: String,
    zmq_sender: &ZmqSender,
) -> Result<(), Error> {
    let (imap_server, imap_port, username, password) =
        match (&hub.imap_server, hub.imap_port, &hub.login, &hub.password) {
            (Some(server), Some(port), Some(username), Some(password)) => {
                (server, port as u16, username, password)
            }
            _ => {
                return Err(Error::Config(format!(
                    "Cannot get imap server and port for the hub#{}",
                    hub.id
                )));
            }
        };

    let mut session = init_session(imap_server, imap_port, username, password).await?;

    let recipients = repo.list_not_replied_email_recipients(hub.id)?;

    log::info!(
        "Found {} recipients for the startup check in hub#{}",
        recipients.len(),
        hub.id,
    );
    for recipient in recipients {
        let in_reply_to_id = format!("<{}@{}>", recipient.id, domain);
        let query = format!("HEADER In-Reply-To {in_reply_to_id}");
        let search_result = match session.uid_search(&query).await {
            Ok(res) => res,
            Err(e) => {
                log::error!("Cannot search for emails in hub#{}: {e}", hub.id);
                continue;
            }
        };

        if let Some(uid) = search_result.iter().max() {
            let reply = fetch_message_body(&mut session, *uid)
                .await
                .map(|b| extract_plain_reply(&b));
            process_reply(&repo, hub.id, &recipient, reply, zmq_sender).await;
        }
    }

    let mut last_uid = session
        .uid_search("ALL")
        .await
        .ok()
        .and_then(|uids| uids.into_iter().max())
        .unwrap_or(0);

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

        let search_query = format!("UID {}:*", last_uid + 1);
        let new_uids = match session.uid_search(&search_query).await {
            Ok(uids) => uids,
            Err(e) => {
                log::error!("Cannot search new emails in hub#{}: {e}", hub.id);
                continue;
            }
        };

        for uid in &new_uids {
            process_new_message(&repo, &mut session, *uid, &domain, hub.id, zmq_sender).await;
        }

        if let Some(max_uid) = new_uids.iter().max() {
            last_uid = *max_uid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_header_values_with_folding() {
        let header = "Subject: Unsubscribe\r\n\trequest\r\nFrom: Name <user@example.com>\r\n";
        assert_eq!(
            extract_header_value(header, "Subject"),
            Some("Unsubscribe request".to_string())
        );
    }

    #[test]
    fn prefers_sender_header_for_email_extraction() {
        let header = "Sender: sender@example.com\r\nFrom: other@example.com\r\n";
        assert_eq!(
            extract_sender_email(header),
            Some("sender@example.com".to_string())
        );
    }

    #[test]
    fn extracts_bounce_recipient_from_body() {
        let body = "Final-Recipient: rfc822; bounced@example.com\nMail delivery failed";
        assert_eq!(
            extract_bounce_recipient(body),
            Some("bounced@example.com".to_string())
        );
    }
}
