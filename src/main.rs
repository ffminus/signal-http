mod client;
mod codec;
mod transport;

use core::error::Error;

use std::sync::Arc;

use clap::Parser;
use color_eyre::eyre::Result;
use jsonrpsee::ws_client::WsClient;
use poem_openapi::payload::Json;
use poem_openapi::{Enum, Object};

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
type ResultPoem<T = ()> = poem::Result<T>;

#[poem_openapi::OpenApi]
impl Api {
    /// Send emoji reaction to a message.
    #[oai(path = "/react", method = "post")]
    async fn react(&self, body: Json<React>, signal: Signal<'_, '_>) -> ResultPoem {
        let (person, group) = parse_recipient(&body.recipient)?;

        signal
            .react(person, group, &body.emoji, &body.author, body.timestamp)
            .await
            .or_internal_server_error()?;

        Ok(())
    }

    /// Send read receipt event.
    #[oai(path = "/receive", method = "post")]
    async fn receive(&self, body: Json<Receive>, signal: Signal<'_, '_>) -> ResultPoem {
        signal
            .receive(&body.recipient, body.timestamp)
            .await
            .or_internal_server_error()?;

        Ok(())
    }

    /// Send a message to `signal-cli` daemon.
    #[oai(path = "/send", method = "post")]
    async fn send(&self, body: Json<Send>, signal: Signal<'_, '_>) -> ResultPoem<Json<SendResp>> {
        use serde_json::from_value;

        let (person, group) = parse_recipient(&body.recipient)?;

        let attachments: Vec<_> = body
            .attachments
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|attachement| format!("data:image/jpeg;base64,{attachement}"))
            .collect();

        let value = signal
            .send(person, group, &body.message, &attachments)
            .await
            .or_internal_server_error()?;

        Ok(Json(from_value(value).or_internal_server_error()?))
    }

    /// Match API of `bbernhard/signal-cli-rest-api` for compatibility.
    #[oai(path = "/v2/send", method = "post")]
    async fn send_compat(
        &self,
        Json(mut b): Json<SendCompat>,
        sig: Signal<'_, '_>,
    ) -> ResultPoem<Json<SendResp>> {
        let Some(recipient) = b.recipients.pop() else {
            return unprocessable("Missing message recipient");
        };

        if !b.recipients.is_empty() {
            return unprocessable("Multi-recipient messages are not supported");
        }

        // Adapt payload to match crate API
        let body = Send {
            message: b.message,
            recipient: Recipient {
                kind: RecipientKind::Person,
                value: recipient,
            },
            attachments: None,
        };

        // Forward call to `send` endpoint to centralize logic
        self.send(Json(body), sig).await
    }

    /// Send a message to `signal-cli` daemon.
    #[oai(path = "/typing", method = "post")]
    async fn typing(&self, b: Json<Typing>, signal: Signal<'_, '_>) -> ResultPoem {
        let (person, group) = parse_recipient(&b.recipient)?;

        signal
            .send_typing(person, group, b.stop)
            .await
            .or_internal_server_error()?;

        Ok(())
    }
}

#[expect(clippy::result_large_err)]
fn parse_recipient(recipient: &Recipient) -> ResultPoem<(Option<&str>, Option<&str>)> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    match recipient.kind {
        RecipientKind::Person => Ok((Some(&recipient.value), None)),
        RecipientKind::Group => {
            let Ok(bytes) = STANDARD.decode(&recipient.value) else {
                return unprocessable("Group id is not valid base64");
            };

            if bytes.len() != 32 {
                return unprocessable("Invalid group id");
            }

            Ok((None, Some(&recipient.value)))
        }
    }
}

#[expect(clippy::result_large_err)]
fn unprocessable<T>(msg: &str) -> ResultPoem<T> {
    use poem::error::Error;
    use poem::http::StatusCode;

    Err(Error::from_string(msg, StatusCode::UNPROCESSABLE_ENTITY))
}

#[derive(Object)]
struct React {
    recipient: Recipient,
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
    recipient: Recipient,
    message: String,
    attachments: Option<Vec<String>>,
}

#[derive(Object, serde::Deserialize)]
struct SendResp {
    timestamp: u64,
}

#[derive(Object)]
struct SendCompat {
    recipients: Vec<String>,
    message: String,
}

#[derive(Object)]
struct Typing {
    recipient: Recipient,
    stop: bool,
}

#[derive(Object)]
struct Recipient {
    kind: RecipientKind,
    value: String,
}

#[derive(Enum)]
#[oai(rename_all(lowercase))]
enum RecipientKind {
    Person,
    Group,
}

trait OrInternalServerError<T> {
    #[expect(clippy::result_large_err)]
    fn or_internal_server_error(self) -> ResultPoem<T>;
}

impl<T, E: 'static + core::marker::Send + Sync + Error> OrInternalServerError<T> for Result<T, E> {
    fn or_internal_server_error(self) -> ResultPoem<T> {
        self.map_err(|error| poem::error::InternalServerError(error))
    }
}
