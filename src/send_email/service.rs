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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use crate::repository::DieselRepository;
    use diesel::{RunQueryDsl, connection::SimpleConnection};
    use pushkind_common::db::establish_connection_pool;
    use pushkind_common::domain::emailer::email::{NewEmail, NewEmailRecipient};
    use pushkind_common::models::emailer::hub::NewHub as DbNewHub;
    use pushkind_common::schema::emailer::hubs;
    use tempfile::TempDir;

    struct MockMailer {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    #[async_trait]
    impl Mailer for MockMailer {
        async fn send(&self, _hub: &Hub, _message: MessageBuilder<'_>) -> Result<(), Error> {
            if self.fail {
                Err(Error::Config("fail".into()))
            } else {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    fn setup_pool() -> (TempDir, pushkind_common::db::DbPool) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = establish_connection_pool(db_path.to_str().unwrap()).unwrap();
        {
            let mut conn = pool.get().unwrap();
            conn.batch_execute(
                "CREATE TABLE hubs (id INTEGER PRIMARY KEY, login TEXT, password TEXT, sender TEXT, smtp_server TEXT, smtp_port INTEGER, created_at TIMESTAMP, updated_at TIMESTAMP, imap_server TEXT, imap_port INTEGER, email_template TEXT, imap_last_uid INTEGER NOT NULL DEFAULT 0);\n\
                 CREATE TABLE emails (id INTEGER PRIMARY KEY, message TEXT NOT NULL, created_at TIMESTAMP NOT NULL, is_sent BOOL NOT NULL, subject TEXT, attachment BLOB, attachment_name TEXT, attachment_mime TEXT, num_sent INTEGER NOT NULL DEFAULT 0, num_opened INTEGER NOT NULL DEFAULT 0, num_replied INTEGER NOT NULL DEFAULT 0, hub_id INTEGER NOT NULL REFERENCES hubs(id));\n\
                CREATE TABLE email_recipients (id INTEGER PRIMARY KEY, email_id INTEGER NOT NULL REFERENCES emails(id), address TEXT NOT NULL, opened BOOL NOT NULL, updated_at TIMESTAMP NOT NULL, is_sent BOOL NOT NULL, replied BOOL NOT NULL, name TEXT, fields TEXT, reply TEXT);"
            ).unwrap();
        }
        (dir, pool)
    }

    fn insert_hub(pool: &pushkind_common::db::DbPool) {
        let mut conn = pool.get().unwrap();
        let hub = DbNewHub {
            id: 1,
            login: Some("sender@example.com"),
            password: Some("pass"),
            sender: Some("sender@example.com"),
            smtp_server: None,
            smtp_port: None,
            created_at: None,
            updated_at: None,
            imap_server: None,
            imap_port: None,
            email_template: Some("Hi {name}! {message}"),
        };
        diesel::insert_into(hubs::table)
            .values(&hub)
            .execute(&mut conn)
            .unwrap();
    }

    fn create_email(repo: &DieselRepository) -> (i32, i32) {
        let new_email = NewEmail {
            message: "Hello".into(),
            subject: None,
            attachment: None,
            attachment_name: None,
            attachment_mime: None,
            hub_id: 1,
            recipients: vec![NewEmailRecipient {
                address: "to@example.com".into(),
                name: "".to_string(),
                fields: HashMap::new(),
            }],
        };
        let stored = repo.create_email(&new_email).unwrap();
        (stored.email.id, stored.recipients[0].id)
    }

    #[tokio::test]
    async fn send_email_updates_recipient_on_success() {
        let (_dir, pool) = setup_pool();
        insert_hub(&pool);
        let repo = DieselRepository::new(pool.clone());
        let (email_id, recipient_id) = create_email(&repo);

        let mailer = MockMailer {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: false,
        };
        let msg = ZMQSendEmailMessage::RetryEmail((email_id, 1));
        send_email(msg, &repo, "example.com", &mailer)
            .await
            .unwrap();
        assert_eq!(mailer.calls.load(Ordering::SeqCst), 1);

        let updated = repo
            .get_email_recipient_by_id(recipient_id, 1)
            .unwrap()
            .unwrap();
        assert!(updated.is_sent);
    }

    #[tokio::test]
    async fn send_email_skips_update_on_failure() {
        let (_dir, pool) = setup_pool();
        insert_hub(&pool);
        let repo = DieselRepository::new(pool.clone());
        let (email_id, recipient_id) = create_email(&repo);

        let mailer = MockMailer {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: true,
        };
        let msg = ZMQSendEmailMessage::RetryEmail((email_id, 1));
        send_email(msg, &repo, "example.com", &mailer)
            .await
            .unwrap();
        assert_eq!(mailer.calls.load(Ordering::SeqCst), 0);

        let updated = repo
            .get_email_recipient_by_id(recipient_id, 1)
            .unwrap()
            .unwrap();
        assert!(!updated.is_sent);
    }
}
