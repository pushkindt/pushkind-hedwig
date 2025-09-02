use std::env;
use std::str;
use std::sync::Arc;

use dotenvy::dotenv;
use pushkind_common::db::establish_connection_pool;
use pushkind_common::domain::emailer::email::EmailRecipient;
use pushkind_common::domain::emailer::email::UpdateEmailRecipient;
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::ZMQReplyMessage;
use pushkind_common::zmq::{ZmqSender, ZmqSenderOptions};
use tokio::task;

use pushkind_hedwig::errors::Error;
use pushkind_hedwig::repository::{DieselRepository, EmailReader, EmailWriter, HubReader};

fn strip_html_tags(input: &str) -> String {
    // Use an HTML parser to safely convert markup into plain text.
    // This avoids edge cases with malformed tags and ensures that the
    // resulting string contains no HTML elements.
    let plain =
        html2text::from_read(input.as_bytes(), usize::MAX).unwrap_or_else(|_| input.to_string());
    plain.replace('\u{00a0}', " ")
}

fn extract_plain_reply(input: &str) -> String {
    // 1) Convert HTML to sanitized plain text
    let sanitized = strip_html_tags(input);
    // 2) Normalize newlines
    let normalized = sanitized.replace('\r', "");
    // 3) Remove quoted lines and cut off at common quote separators
    let mut result_lines = Vec::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Keep single blank lines, but avoid leading whitespace-only content
            if !result_lines.is_empty() {
                result_lines.push(String::new());
            }
            continue;
        }

        // Stop at typical reply separators
        let lower = trimmed.to_lowercase();
        let is_gmail_sep = lower.starts_with("on ") && lower.ends_with(" wrote:");
        let is_original_msg = lower.contains("original message")
            || lower.contains("пересылаемое сообщение")
            || lower.contains("исходное сообщение");
        let is_header_block = lower.starts_with("from:")
            || lower.starts_with("от кого:")
            || lower.starts_with("subject:")
            || lower.starts_with("тема:")
            || lower.starts_with("to:")
            || lower.starts_with("кому:")
            || lower.starts_with("date:")
            || lower.starts_with("дата:");

        if is_gmail_sep || is_original_msg {
            break;
        }
        if is_header_block && !result_lines.is_empty() {
            // If we already captured something, hitting a header likely indicates quoted section
            break;
        }
        // Skip quoted lines that begin with '>'
        if trimmed.starts_with('>') {
            continue;
        }
        result_lines.push(trimmed.to_string());
    }

    let mut reply = result_lines.join("\n");
    reply = reply.trim().to_string();

    if reply.is_empty() {
        // Fallback: take the first non-empty, non-quote paragraph
        for para in normalized.split("\n\n") {
            let p = para
                .lines()
                .filter(|l| !l.trim().starts_with('>'))
                .collect::<Vec<_>>()
                .join("\n");
            let p = p.trim();
            if !p.is_empty() {
                reply = p.to_string();
                break;
            }
        }
    }
    reply
}

fn extract_recipient_id(header: &str, domain: &str) -> Option<i32> {
    header
        .lines()
        .find(|line| line.starts_with("In-Reply-To:"))
        .and_then(|line| line.split('<').nth(1))
        .and_then(|part| part.split('>').next())
        .and_then(|msg_id| {
            let mut parts = msg_id.split('@');
            match (parts.next(), parts.next()) {
                (Some(id), Some(d)) if d == domain => id.parse().ok(),
                _ => None,
            }
        })
}

fn fetch_message_body(
    session: &mut imap::Session<impl std::io::Read + std::io::Write>,
    uid: u32,
) -> Option<String> {
    let fetches = match session.uid_fetch(uid.to_string(), "RFC822.TEXT") {
        Ok(f) => f,
        Err(e) => {
            log::error!("Cannot fetch body for UID {uid}: {e}");
            return None;
        }
    };

    let fetch = fetches.iter().next()?;
    let body = fetch.text().or_else(|| fetch.body())?;

    match str::from_utf8(body) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            log::error!("Cannot parse body utf8 for UID {uid}: {e}");
            None
        }
    }
}

fn process_reply(
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

fn process_new_message(
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

fn monitor_hub(repo: DieselRepository, hub: Hub, domain: String, zmq_sender: &ZmqSender) {
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

    let tls = match native_tls::TlsConnector::builder().build() {
        Ok(tls) => tls,
        Err(e) => {
            log::error!("Cannot build tls connector for hub#{}: {e}", hub.id);
            return;
        }
    };
    let client = match imap::connect((imap_server.as_str(), imap_port), imap_server, &tls) {
        Ok(client) => client,
        Err(e) => {
            log::error!("Cannot connect to imap server in hub#{}: {e}", hub.id);
            return;
        }
    };

    let mut session = match client.login(username, password).map_err(|e| e.0) {
        Ok(session) => session,
        Err(e) => {
            log::error!("Cannot login to imap server in hub#{}: {e}", hub.id);
            return;
        }
    };

    if let Err(e) = session.select("INBOX") {
        log::error!("Cannot select INBOX in hub#{}: {e}", hub.id);
        return;
    }

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

async fn run() -> Result<(), Error> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "app.db".to_string());
    let domain = Arc::new(env::var("DOMAIN").unwrap_or_default());
    let zmq_address = env::var("ZMQ_REPLIER_PUB").unwrap_or("tcp://127.0.0.1:5559".to_string());

    let db_pool = establish_connection_pool(&database_url)?;
    let repo = DieselRepository::new(db_pool);

    let zmq_sender = Arc::new(ZmqSender::start(ZmqSenderOptions::pub_default(
        &zmq_address,
    )));

    let hubs = repo.list_hubs()?;

    let mut handles = vec![];
    for hub in hubs {
        let repo = repo.clone();
        let domain = Arc::clone(&domain);
        let zmq_sender = zmq_sender.clone();
        handles.push(task::spawn_blocking(move || {
            monitor_hub(repo, hub, domain.to_string(), &zmq_sender)
        }));
    }

    for handle in handles {
        if let Err(e) = handle.await {
            log::error!("Task panicked: {e:?}");
        }
    }

    Ok(())
}

/// Entry point for the reply-checking worker.
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        log::error!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_text_from_html() {
        let html = "<div>Hello <b>world</b></div>";
        assert_eq!(extract_plain_reply(html), "Hello world");
    }

    #[test]
    fn ignores_quoted_sections() {
        let html = "<div>Thanks!</div><div><br></div><div>On Tue, Someone wrote:</div><blockquote><div>Original</div></blockquote>";
        assert_eq!(extract_plain_reply(html), "Thanks!");
    }
}
