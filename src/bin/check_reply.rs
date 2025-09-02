use std::env;
use std::sync::Arc;

use dotenvy::dotenv;
use pushkind_common::db::establish_connection_pool;
use pushkind_common::zmq::{ZmqSender, ZmqSenderOptions};
use pushkind_hedwig::check_reply;
use pushkind_hedwig::repository::DieselRepository;

/// Entry point for the reply-checking worker.
#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "app.db".to_string());
    let domain = env::var("DOMAIN").unwrap_or_default();
    let zmq_address = env::var("ZMQ_REPLIER_PUB").unwrap_or("tcp://127.0.0.1:5559".to_string());

    let db_pool = establish_connection_pool(&database_url).expect("Failed to connect to DB");
    let repo = DieselRepository::new(db_pool);
    let zmq_sender = Arc::new(ZmqSender::start(ZmqSenderOptions::pub_default(
        &zmq_address,
    )));

    if let Err(e) = check_reply::run(repo, domain, zmq_sender).await {
        log::error!("{e}");
        std::process::exit(1);
    }
}
