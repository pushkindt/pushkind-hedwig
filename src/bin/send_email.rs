use pushkind_hedwig::send_email::run;

/// Entry point for the email sender worker.
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        log::error!("{e}");
        std::process::exit(1);
    }
}
