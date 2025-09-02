pub mod imap;
pub mod parser;
pub mod processor;

use std::sync::Arc;

use crate::errors::Error;
use crate::repository::{DieselRepository, HubReader};
use pushkind_common::zmq::ZmqSender;
use tokio::task;

/// Run the reply monitoring worker.
pub async fn run(
    repo: DieselRepository,
    domain: String,
    zmq_sender: Arc<ZmqSender>,
) -> Result<(), Error> {
    let domain = Arc::new(domain);
    let hubs = repo.list_hubs()?;
    let mut handles = vec![];
    for hub in hubs {
        let repo = repo.clone();
        let domain = Arc::clone(&domain);
        let zmq_sender = zmq_sender.clone();
        handles.push(task::spawn_blocking(move || {
            processor::monitor_hub(repo, hub, domain.to_string(), &zmq_sender)
        }));
    }

    for handle in handles {
        if let Err(e) = handle.await {
            log::error!("Task panicked: {e:?}");
        }
    }

    Ok(())
}
