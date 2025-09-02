use std::env;

use dotenvy::dotenv;
use pushkind_hedwig::send_email;

/// Entry point for the email sender worker.
#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or("app.db".to_string());
    let domain = env::var("DOMAIN").unwrap_or_default();
    let zmq_address = env::var("ZMQ_EMAILER_SUB").unwrap_or("tcp://127.0.0.1:5558".to_string());

    if let Err(e) = send_email::run(&database_url, &domain, &zmq_address).await {
        log::error!("{e}");
        std::process::exit(1);
    }
}
