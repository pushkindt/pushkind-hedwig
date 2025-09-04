use std::env;

use dotenvy::dotenv;
use pushkind_hedwig::check_reply;

/// Entry point for the reply-checking worker.
#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();
    rustls::crypto::CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider())
        .expect("Could not install default crypto provider.");

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "app.db".to_string());
    let domain = env::var("DOMAIN").unwrap_or_default();
    let zmq_address = env::var("ZMQ_REPLIER_PUB").unwrap_or("tcp://127.0.0.1:5559".to_string());

    if let Err(e) = check_reply::run(&database_url, &domain, &zmq_address).await {
        log::error!("{e}");
        std::process::exit(1);
    }
}
