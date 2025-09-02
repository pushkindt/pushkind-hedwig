use async_trait::async_trait;
use mail_send::mail_builder::MessageBuilder;
use pushkind_common::domain::emailer::email::UpdateEmailRecipient;
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::ZMQSendEmailMessage;

use crate::errors::Error;
use crate::repository::{EmailReader, EmailWriter, HubReader};

use super::message_builder::build_message;

/// Abstraction over message delivery.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Sends the provided message using SMTP credentials from the hub.
    async fn send(&self, hub: &Hub, message: MessageBuilder<'_>) -> Result<(), Error>;
}

/// Processes a [`ZMQSendEmailMessage`] by fetching data from the repository
/// and dispatching email messages via the provided [`Mailer`].
pub async fn send_email<R, M>(
    msg: ZMQSendEmailMessage,
    repo: &R,
    domain: &str,
    mailer: &M,
) -> Result<(), Error>
where
    R: EmailReader + EmailWriter + HubReader,
    M: Mailer,
{
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

        let message = build_message(&hub, &email.email, &recipient, domain);

        if let Err(e) = mailer.send(&hub, message).await {
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
