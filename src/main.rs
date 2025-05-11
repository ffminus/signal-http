use clap::Parser;
use color_eyre::eyre::Result;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// address of `signal-cli` daemon
    #[arg(long)]
    daemon: String,

    /// endpoint to forward messages to
    #[arg(long)]
    webhook: String,

    /// external URL service can be accessed from
    #[arg(long, default_value = "http://localhost")]
    url: String,

    /// host to bind HTTP server to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// port to bind HTTP server to
    #[arg(long, default_value = "80")]
    port: u16,
}

fn main() -> Result<()> {
    // Initialize logs and traces consumer
    tracing_subscriber::fmt::init();

    // Create async runtime
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(main_async(Args::parse()))
}

async fn main_async(_args: Args) -> Result<()> {
    Ok(())
}
