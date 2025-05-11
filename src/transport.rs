use std::io::Result as ResultIo;

use futures_util::SinkExt;
use jsonrpsee::core::client::{ReceivedMessage, TransportReceiverT, TransportSenderT};

pub struct Sender<T>(T);

impl<T> Sender<T> {
    pub const fn new(sink: T) -> Self {
        Self(sink)
    }
}

impl<T: 'static + Send + Unpin + futures_util::Sink<String, Error = impl core::error::Error>>
    TransportSenderT for Sender<T>
{
    type Error = Error;

    async fn send(&mut self, body: String) -> Result<(), Self::Error> {
        self.0.send(body).await.map_err(Error::from_error)
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.0.close().await.map_err(Error::from_error)
    }
}

pub struct Receiver<T>(T);

impl<T> Receiver<T> {
    pub const fn new(stream: T) -> Self {
        Self(stream)
    }
}

impl<T: 'static + Send + Unpin + futures_util::Stream<Item = ResultIo<String>>> TransportReceiverT
    for Receiver<T>
{
    type Error = Error;

    async fn receive(&mut self) -> Result<ReceivedMessage, Self::Error> {
        use futures_util::stream::StreamExt;

        let Some(result) = self.0.next().await else {
            return Err(Error(String::from("Closed")));
        };

        Ok(ReceivedMessage::Text(result.map_err(Error::from_error)?))
    }
}

#[derive(Debug)]
pub struct Error(String);

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "Errors: {}", self.0)
    }
}

impl core::error::Error for Error {}

impl Error {
    fn from_error(error: impl core::error::Error) -> Self {
        Self(format!("{error:?}"))
    }
}
