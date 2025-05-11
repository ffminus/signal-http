use std::io::{Error as ErrorIo, Result as ResultIo};

use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

pub struct Codec;

impl Decoder for Codec {
    type Item = String;
    type Error = ErrorIo;

    fn decode(&mut self, buf: &mut BytesMut) -> ResultIo<Option<Self::Item>> {
        let Some(i) = buf.as_ref().iter().position(|&b| b == b'\n') else {
            return Ok(None);
        };

        let line = buf.split_to(i);
        let _ = buf.split_to(1);

        let Ok(s) = core::str::from_utf8(line.as_ref()) else {
            return Err(ErrorIo::other("invalid UTF-8"));
        };

        Ok(Some(s.to_string()))
    }
}

impl Encoder<String> for Codec {
    type Error = ErrorIo;

    fn encode(&mut self, msg: String, buf: &mut BytesMut) -> ResultIo<()> {
        let mut payload = msg.into_bytes();
        payload.push(b'\n');
        buf.extend_from_slice(&payload);
        Ok(())
    }
}
