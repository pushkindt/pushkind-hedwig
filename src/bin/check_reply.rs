use std::env;

use config::Config;
use dotenvy::dotenv;

use pushkind_hedwig::{check_reply, models::ServerConfig};

/// Entry point for the reply-checking worker.
#[tokio::main]
async fn main() {
    // Load environment variables from `.env` in local development.
    dotenv().ok();
    // Initialize logger with default level INFO if not provided.
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    rustls::crypto::CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider())
        .expect("Could not install default crypto provider.");

    // Select config profile (defaults to `local`).
    let app_env = env::var("APP_ENV").unwrap_or_else(|_| "local".into());

    let settings = Config::builder()
        // Add `./config/default.yaml`
        .add_source(config::File::with_name("config/default"))
        // Add environment-specific overrides
        .add_source(config::File::with_name(&format!("config/{}", app_env)).required(false))
        // Add settings from the environment (with a prefix of APP)
        .add_source(config::Environment::with_prefix("APP"))
        .build();

    let settings = match settings {
        Ok(settings) => settings,
        Err(err) => {
            log::error!("Error loading settings: {}", err);
            std::process::exit(1);
        }
    };

    let server_config = match settings.try_deserialize::<ServerConfig>() {
        Ok(server_config) => server_config,
        Err(err) => {
            log::error!("Error loading server config: {}", err);
            std::process::exit(1);
        }
    };

    if let Err(e) = check_reply::run(
        &server_config.database_url,
        &server_config.domain,
        &server_config.zmq_replier_pub,
    )
    .await
    {
        log::error!("{e}");
        std::process::exit(1);
    }
}
