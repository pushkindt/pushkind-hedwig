pub mod imap;
pub mod parser;
pub mod service;

use std::sync::Arc;
use std::time::Duration;

use pushkind_common::db::establish_connection_pool;
use pushkind_common::zmq::{ZmqSender, ZmqSenderOptions};
use tokio::task::JoinSet;

use crate::check_reply::service::monitor_hub;
use crate::errors::Error;
use crate::repository::{DieselRepository, HubReader};

/// Run the reply monitoring worker.
pub async fn run(database_url: &str, domain: &str, zmq_address: &str) -> Result<(), Error> {
    let db_pool = establish_connection_pool(database_url)?;
    let repo = DieselRepository::new(db_pool);

    let zmq_sender = ZmqSender::start(ZmqSenderOptions::pub_default(zmq_address))?;
    let zmq_sender = Arc::new(zmq_sender);

    let domain = Arc::new(domain.to_owned());
    let hubs = repo.list_hubs()?;
    let mut join_set = JoinSet::new();

    log::info!("Starting email checking worker");

    for hub in hubs {
        let repo = repo.clone();
        let domain = Arc::clone(&domain);
        let zmq_sender = zmq_sender.clone();
        let hub_id = hub.id;
        join_set.spawn(async move {
            log::info!("Starting monitor loop for hub#{}", hub_id);
            loop {
                // Always fetch the latest hub config before each attempt
                let hub_opt = match repo.get_hub_by_id(hub_id) {
                    Ok(h) => h,
                    Err(e) => {
                        log::error!("Failed to fetch hub#{} config: {}", hub_id, e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let Some(hub) = hub_opt else {
                    log::warn!("Hub#{} not found. Will retry soon…", hub_id);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                };

                // Run hub monitor in a child task to catch panics via JoinError
                let repo_for_task = repo.clone();
                let domain_for_task = domain.to_string();
                let zmq_for_task = zmq_sender.clone();
                let handle = tokio::spawn(async move {
                    monitor_hub(repo_for_task, hub, domain_for_task, &zmq_for_task).await
                });

                match handle.await {
                    Ok(Ok(())) => {
                        log::info!("monitor_hub completed for hub#{}", hub_id);
                        break;
                    }
                    Ok(Err(e)) => {
                        log::error!(
                            "monitor_hub failed for hub#{}: {} — restarting soon",
                            hub_id,
                            e
                        );
                    }
                    Err(e) => {
                        log::error!(
                            "monitor_hub panicked for hub#{}: {:?} — restarting soon",
                            hub_id,
                            e
                        );
                    }
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    // Drain join handles; monitor tasks self-restart on failure
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(()) => {
                log::info!("A monitor task exited cleanly");
            }
            Err(e) => {
                log::error!("A monitor task join error: {e:?}");
            }
        }
    }

    Ok(())
}
