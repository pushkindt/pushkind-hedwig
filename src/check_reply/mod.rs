pub mod imap;
pub mod parser;
pub mod service;

use std::sync::Arc;

use pushkind_common::db::establish_connection_pool;
use pushkind_common::zmq::{ZmqSender, ZmqSenderOptions};
use tokio::task;

use crate::check_reply::service::monitor_hub;
use crate::errors::Error;
use crate::repository::{DieselRepository, HubReader};

/// Run the reply monitoring worker.
pub async fn run(database_url: &str, domain: &str, zmq_address: &str) -> Result<(), Error> {
    let db_pool = establish_connection_pool(database_url)?;
    let repo = DieselRepository::new(db_pool);

    let zmq_sender = Arc::new(ZmqSender::start(ZmqSenderOptions::pub_default(zmq_address)));

    let domain = Arc::new(domain.to_owned());
    let hubs = repo.list_hubs()?;
    let mut handles = vec![];

    log::info!("Starting email checking worker");

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
