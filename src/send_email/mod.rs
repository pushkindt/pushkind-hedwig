pub mod message_builder;
pub mod service;

use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use dotenvy::dotenv;
use mail_send::SmtpClientBuilder;
use mail_send::mail_builder::MessageBuilder;
use pushkind_common::db::establish_connection_pool;
use pushkind_common::domain::emailer::hub::Hub;
use pushkind_common::models::emailer::zmq::ZMQSendEmailMessage;

use crate::errors::Error;
use crate::repository::DieselRepository;

use service::{Mailer, send_email};

/// Simple SMTP mailer that leverages [`mail_send`].
pub struct SmtpMailer;

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, hub: &Hub, message: MessageBuilder<'_>) -> Result<(), Error> {
        let smtp_server = hub.smtp_server.as_deref().unwrap_or_default();
        let smtp_port = hub.smtp_port.unwrap_or(25) as u16;
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
}

/// Entry point for the email sender worker.
pub async fn run() -> Result<(), Error> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();

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
