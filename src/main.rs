mod client;
mod codec;
mod transport;

use std::sync::Arc;

use clap::Parser;
use color_eyre::eyre::Result;
use jsonrpsee::ws_client::WsClient;
use poem::error::InternalServerError;
use poem_openapi::Object;
use poem_openapi::payload::Json;

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

    // Listen to HTTP requests too
    serve(signal, args.url, args.host, args.port).await
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

/// Handle incoming HTTP requests.
async fn serve(signal: Arc<WsClient>, url: String, host: String, port: u16) -> Result<()> {
    use poem::middleware::AddData;
    use poem::{EndpointExt, Route, Server};

    /// Pull crate name from environment variable at compile time.
    const NAME: &str = env!("CARGO_PKG_NAME");

    // Describe API routes and endpoints according to OpenAPI spec
    let app = poem_openapi::OpenApiService::new(Api, NAME, env!("CARGO_PKG_VERSION")).server(url);

    // Host documentation on dedicated page
    let docs = app.swagger_ui();

    // Associate routes with handler functions, store daemon connection in application state
    let router = Route::new()
        .nest("/", app)
        .nest("/docs", docs)
        .with(AddData::new(signal));

    // Listen to incoming requests, bind to address specified by caller
    Ok(Server::new(poem::listener::TcpListener::bind((host, port)))
        .name(NAME)
        .run(router)
        .await?)
}

/// Proxy to interact with Signal service.
type Signal<'a, 'p> = poem::web::Data<&'a Arc<WsClient>>;

/// Attach endpoint handlers to dummy struct to generate documentation automatically.
struct Api;

/// Empty response of fallible handler.
type ResultPoem = poem::Result<()>;

#[poem_openapi::OpenApi]
impl Api {
    /// Send emoji reaction to a message.
    #[oai(path = "/react", method = "post")]
    async fn react(&self, Json(body): Json<React>, signal: Signal<'_, '_>) -> ResultPoem {
        validate_recipients(body.recipient.as_deref(), body.group.as_deref())?;

        signal
            .react(
                body.recipient.as_deref(),
                body.group.as_deref(),
                &body.emoji,
                &body.author,
                body.timestamp,
            )
            .try_into_empty_response()
            .await
    }

    /// Send read receipt event.
    #[oai(path = "/receive", method = "post")]
    async fn receive(&self, Json(body): Json<Receive>, signal: Signal<'_, '_>) -> ResultPoem {
        signal
            .receive(&body.recipient, body.timestamp)
            .try_into_empty_response()
            .await
    }

    /// Send a message to `signal-cli` daemon.
    #[oai(path = "/send", method = "post")]
    async fn send(&self, Json(body): Json<Send>, signal: Signal<'_, '_>) -> ResultPoem {
        validate_recipients(body.recipient.as_deref(), body.group.as_deref())?;

        let attachments: Vec<_> = body
            .attachments
            .unwrap_or_else(Vec::new)
            .iter()
            .map(|attachement| format!("data:image/jpeg;base64,{attachement}"))
            .collect();

        signal
            .send(
                body.recipient.as_deref(),
                body.group.as_deref(),
                &body.message,
                &attachments,
            )
            .try_into_empty_response()
            .await
    }
}

#[expect(clippy::result_large_err)]
fn validate_recipients(recipient: Option<&str>, group: Option<&str>) -> ResultPoem {
    use base64::Engine;

    let fail = |m| poem::error::Error::from_string(m, poem::http::StatusCode::UNPROCESSABLE_ENTITY);

    if recipient.is_none() && group.is_none() {
        return Err(fail("Provide a recipient or group"));
    }

    if let Some(group) = group {
        if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(group) {
            if bytes.len() != 32 {
                return Err(fail("Invalid group id"));
            }
        }
    }

    Ok(())
}

#[derive(Object)]
struct React {
    recipient: Option<String>,
    group: Option<String>,
    emoji: String,
    author: String,
    timestamp: u64,
}

#[derive(Object)]
struct Receive {
    recipient: String,
    timestamp: u64,
}

#[derive(Object)]
struct Send {
    recipient: Option<String>,
    group: Option<String>,
    message: String,
    attachments: Option<Vec<String>>,
}

trait TryIntoEmptyResponse {
    async fn try_into_empty_response(self) -> ResultPoem;
}

impl<T, F: Future<Output = Result<T, jsonrpsee::core::client::Error>>> TryIntoEmptyResponse for F {
    async fn try_into_empty_response(self) -> ResultPoem {
        if let Err(error) = self.await {
            Err(InternalServerError(error))
        } else {
            Ok(())
        }
    }
}
