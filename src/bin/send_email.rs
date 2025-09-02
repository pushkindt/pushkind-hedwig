use std::env;
use std::sync::Arc;

use dotenvy::dotenv;
use mail_send::SmtpClientBuilder;
use mail_send::mail_builder::{
    MessageBuilder,
    headers::{HeaderType, url::URL},
};
use pushkind_common::db::establish_connection_pool;
use pushkind_common::domain::emailer::email::{Email, EmailRecipient, UpdateEmailRecipient};
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::ZMQSendEmailMessage;

use pushkind_hedwig::errors::Error;
use pushkind_hedwig::repository::{DieselRepository, EmailReader, EmailWriter, HubReader};

async fn send_smtp_message(
    hub: &Hub,
    email: &Email,
    recipient: &EmailRecipient,
    domain: &str,
) -> Result<(), Error> {
    let template = hub.email_template.as_deref().unwrap_or_default();

    let unsubscribe_url = hub.unsubscribe_url();
    let mut body: String;

    let template = template
        .replace("{unsubscribe_url}", &unsubscribe_url)
        .replace("{name}", recipient.name.as_deref().unwrap_or_default());

    if template.contains("{message}") {
        body = template.replace("{message}", &email.message);
    } else {
        body = format!("{}{}", &email.message, template);
    }

    body.push_str(&format!(
        r#"<img height="1" width="1" border="0" src="https://mail.{domain}/track/{}">"#,
        recipient.id
    ));

    let message_id = format!("{}@{}", recipient.id, domain);

    let recipient_address = vec![("", recipient.address.as_str())];
    let sender_email = hub.sender.as_deref().unwrap_or_default();
    let sender_login = hub.login.as_deref().unwrap_or_default();
    let subject = email.subject.as_deref().unwrap_or_default();

    let mut message = MessageBuilder::new()
        .from((sender_email, sender_login))
        .to(recipient_address)
        .subject(subject)
        .html_body(&body)
        .text_body(&body)
        .message_id(message_id)
        .header(
            "List-Unsubscribe",
            HeaderType::from(URL::new(&unsubscribe_url)),
        );

    if let (Some(mime), Some(name), Some(content)) = (
        email.attachment_mime.as_deref(),
        email.attachment_name.as_deref(),
        email.attachment.as_deref(),
    ) && !name.is_empty()
        && !content.is_empty()
    {
        message = message.attachment(mime, name, content);
    }

    let smtp_server = hub.smtp_server.as_deref().unwrap_or_default();
    let smtp_port = hub.smtp_port.unwrap_or(25) as u16; // assume smtp_port is Option<u16>?

    let credentials = (
        hub.login.as_deref().unwrap_or_default(),
        hub.password.as_deref().unwrap_or_default(),
    );

    SmtpClientBuilder::new(smtp_server, smtp_port)
        .implicit_tls(true)
        .credentials(credentials)
        .connect()
        .await?
        .send(message)
        .await?;
    Ok(())
}

async fn send_email(
    msg: ZMQSendEmailMessage,
    repo: DieselRepository,
    domain: &str,
) -> Result<(), Error> {
    let email = match msg {
        ZMQSendEmailMessage::RetryEmail((email_id, hub_id)) => {
            match repo.get_email_by_id(email_id, hub_id)? {
                Some(email) => email,
                None => {
                    log::error!("Email not found for email_id: {email_id}");
                    return Err(Error::Config("email not found".into()));
                }
            }
        }
        ZMQSendEmailMessage::NewEmail(boxed) => {
            let (_user, new_email) = *boxed;
            repo.create_email(&new_email)?
        }
    };

    let hub = match repo.get_hub_by_id(email.email.hub_id)? {
        Some(hub) => hub,
        None => {
            log::error!("Hub not found for email_id: {}", email.email.id);
            return Ok(());
        }
    };

    log::info!(
        "Sending email for email_id {} via hub {}",
        email.email.id,
        hub.id
    );

    for recipient in email.recipients {
        if recipient.is_sent {
            log::info!("Skipping already sent email to {}", recipient.address);
            continue;
        }

        if let Err(e) = send_smtp_message(&hub, &email.email, &recipient, domain).await {
            log::error!("Failed to send email to {}: {}", recipient.address, e);
            continue;
        }

        log::info!("Email sent successfully to {}", recipient.address);

        if let Err(e) = repo.update_recipient(
            recipient.id,
            &UpdateEmailRecipient {
                is_sent: Some(true),
                replied: None,
                opened: None,
                reply: None,
            },
        ) {
            log::error!(
                "Failed to update sent status for recipient {}: {}",
                recipient.id,
                e
            );
        }
    }

    log::info!("Finished processing email_id: {}", email.email.id);

    Ok(())
}

async fn run() -> Result<(), Error> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok(); // Load .env file

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "app.db".to_string());
    let domain = Arc::from(env::var("DOMAIN").unwrap_or_default());

    let zmq_address =
        env::var("ZMQ_EMAILER_SUB").unwrap_or_else(|_| "tcp://127.0.0.1:5558".to_string());
    let context = zmq::Context::new();
    let responder = context.socket(zmq::SUB)?;
    responder.connect(&zmq_address)?;
    responder.set_subscribe(b"")?;

    let pool = establish_connection_pool(&database_url)?;
    let repo = DieselRepository::new(pool);

    log::info!("Starting email worker");

    loop {
        let msg = responder.recv_bytes(0)?;
        match serde_json::from_slice::<ZMQSendEmailMessage>(&msg) {
            Ok(parsed) => {
                let domain = Arc::clone(&domain);
                let repo = repo.clone();

                tokio::spawn(async move {
                    if let Err(e) = send_email(parsed, repo, &domain).await {
                        log::error!("Error sending email message: {e}");
                    }
                });
            }
            Err(e) => {
                log::error!("Error receiving message: {e}");
            }
        }
    }
}

/// Entry point for the email sender worker.
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        log::error!("{e}");
        std::process::exit(1);
    }
}
