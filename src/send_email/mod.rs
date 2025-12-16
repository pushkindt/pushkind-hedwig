pub mod message_builder;
pub mod service;

use std::sync::Arc;

use async_trait::async_trait;
use mail_send::SmtpClientBuilder;
use mail_send::mail_builder::MessageBuilder;
use pushkind_common::db::establish_connection_pool;
use pushkind_emailer::domain::hub::Hub;
use pushkind_emailer::models::zmq::ZMQSendEmailMessage;

use crate::errors::Error;
use crate::repository::DieselRepository;

use service::{Mailer, send_email};

/// Simple SMTP mailer that leverages [`mail_send`].
pub struct SmtpMailer;

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, hub: &Hub, message: MessageBuilder<'_>) -> Result<(), Error> {
        let smtp_server = hub
            .smtp_server
            .as_ref()
            .map(|host| host.as_str())
            .ok_or(Error::Config("Missed SMTP server address".to_owned()))?;
        let smtp_port = hub
            .smtp_port
            .ok_or(Error::Config("Missed SMTP port".to_owned()))?
            .get();
        let credentials = (
            hub.login
                .as_ref()
                .map(|login| login.as_str())
                .unwrap_or_default(),
            hub.password
                .as_ref()
                .map(|password| password.as_str())
                .unwrap_or_default(),
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
}

/// Entry point for the email sender worker.
pub async fn run(database_url: &str, domain: &str, zmq_address: &str) -> Result<(), Error> {
    let db_pool = establish_connection_pool(database_url)?;
    let repo = DieselRepository::new(db_pool);

    let context = zmq::Context::new();
    let responder = context.socket(zmq::SUB)?;
    responder.connect(zmq_address)?;
    responder.set_subscribe(b"")?;

    let domain = Arc::new(domain.to_owned());

    log::info!("Starting email sending worker");

    loop {
        let msg = responder.recv_bytes(0)?;
        match serde_json::from_slice::<ZMQSendEmailMessage>(&msg) {
            Ok(parsed) => {
                let domain = Arc::clone(&domain);
                let repo = repo.clone();
                tokio::spawn(async move {
                    let mailer = SmtpMailer;
                    if let Err(e) = send_email(parsed, &repo, &domain, &mailer).await {
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
