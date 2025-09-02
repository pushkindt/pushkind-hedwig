use std::str;

use pushkind_common::domain::emailer::email::{EmailRecipient, UpdateEmailRecipient};
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::ZMQReplyMessage;
use pushkind_common::zmq::ZmqSender;

use crate::repository::{DieselRepository, EmailReader, EmailWriter};

use super::imap::{fetch_message_body, init_session};
use super::parser::{extract_plain_reply, extract_recipient_id};

pub fn process_reply(
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
    if let Err(e) = tokio::runtime::Handle::current().block_on(zmq_sender.send_json(&msg)) {
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

pub fn process_new_message(
    repo: &DieselRepository,
    session: &mut imap::Session<impl std::io::Read + std::io::Write>,
    uid: u32,
    domain: &str,
    hub_id: i32,
    zmq_sender: &ZmqSender,
) {
    let fetches = match session.uid_fetch(uid.to_string(), "RFC822.HEADER") {
        Ok(f) => f,
        Err(e) => {
            log::error!("Cannot fetch header for UID {uid}: {e}");
            return;
        }
    };

    let fetch = match fetches.iter().next() {
        Some(f) => f,
        None => return,
    };

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

    if let Some(recipient_id) = extract_recipient_id(header_str, domain) {
        let reply = fetch_message_body(session, uid).map(|b| extract_plain_reply(&b));
        match repo.get_email_recipient_by_id(recipient_id, hub_id) {
            Ok(Some(recipient)) => process_reply(repo, hub_id, &recipient, reply, zmq_sender),
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

pub fn monitor_hub(repo: DieselRepository, hub: Hub, domain: String, zmq_sender: &ZmqSender) {
    let (imap_server, imap_port, username, password) =
        match (&hub.imap_server, hub.imap_port, &hub.login, &hub.password) {
            (Some(server), Some(port), Some(username), Some(password)) => {
                (server, port as u16, username, password)
            }
            _ => {
                log::error!("Cannot get imap server and port for the hub#{}", hub.id);
                return;
            }
        };

    let mut session = match init_session(imap_server, imap_port, username, password) {
        Some(s) => s,
        None => return,
    };

    let recipients = match repo.list_not_replied_email_recipients(hub.id) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Cannot get recipients in hub#{}: {e}", hub.id);
            Vec::new()
        }
    };

    log::info!(
        "Found {} recipients for the startup check in hub#{}",
        recipients.len(),
        hub.id,
    );
    for recipient in recipients {
        let in_reply_to_id = format!("<{}@{}>", recipient.id, domain);
        let query = format!("HEADER In-Reply-To {in_reply_to_id}");
        let search_result = match session.uid_search(&query) {
            Ok(res) => res,
            Err(e) => {
                log::error!("Cannot search for emails in hub#{}: {e}", hub.id);
                continue;
            }
        };

        if let Some(uid) = search_result.iter().max() {
            let reply = fetch_message_body(&mut session, *uid).map(|b| extract_plain_reply(&b));
            process_reply(&repo, hub.id, &recipient, reply, zmq_sender);
        }
    }

    let mut last_uid = session
        .uid_search("ALL")
        .ok()
        .and_then(|uids| uids.into_iter().max())
        .unwrap_or(0);

    log::info!("Starting a monitoring loop for hub#{}", hub.id);
    loop {
        if let Err(e) = session.idle().and_then(|idle| idle.wait_keepalive()) {
            log::error!("Idle error in hub#{}: {e}", hub.id);
            break;
        }

        let search_query = format!("UID {}:*", last_uid + 1);
        let new_uids = match session.uid_search(&search_query) {
            Ok(uids) => uids,
            Err(e) => {
                log::error!("Cannot search new emails in hub#{}: {e}", hub.id);
                continue;
            }
        };

        for uid in &new_uids {
            process_new_message(&repo, &mut session, *uid, &domain, hub.id, zmq_sender);
        }

        if let Some(max_uid) = new_uids.iter().max() {
            last_uid = *max_uid;
        }
    }

    if let Err(e) = session.logout() {
        log::error!("Cannot logout from hub#{}: {e}", hub.id);
    }
}
