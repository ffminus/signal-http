mod client;
mod codec;
mod transport;

use std::sync::Arc;

use clap::Parser;
use color_eyre::eyre::Result;
use jsonrpsee::ws_client::WsClient;

use self::client::SignalClient as Client;

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

async fn main_async(args: Args) -> Result<()> {
    // Interface to communicate with `signal-cli` daemon over JSON-RPC
    let signal = Arc::new(connect(&args.daemon).await?);

    // Listen to incoming messages from daemon
    tokio::spawn(forward_signals(args.webhook, Arc::clone(&signal)));

    Ok(())
}

/// Establish JSON-RPC connection to `signal-cli` daemon.
async fn connect(addr: &str) -> Result<WsClient> {
    use futures_util::stream::StreamExt;
    use jsonrpsee::async_client::ClientBuilder;
    use tokio::net::TcpStream;
    use tokio_util::codec::Decoder;

    use self::transport::{Receiver, Sender};

    let (sink, stream) = codec::Codec.framed(TcpStream::connect(addr).await?).split();

    Ok(ClientBuilder::default().build_with_tokio(Sender::new(sink), Receiver::new(stream)))
}

/// Forward received messages to provided HTTP endpoint.
async fn forward_signals(webhook: String, signal: Arc<WsClient>) -> Result<()> {
    let client = reqwest::Client::new();

    // Listen for incoming messages
    let mut stream = signal.subscribe_receive().await?;

    // Iterate over messages as they arrive
    while let Some(event) = stream.next().await {
        // Forward event wholesale to provided endpoint
        let resp: Result<_> = async { Ok(client.post(&webhook).json(&event?).send().await?) }.await;

        if let Err(error) = resp {
            tracing::warn!("{error}");
        }
    }

    // Notify daemon on unexpected crash
    Ok(stream.unsubscribe().await?)
}
