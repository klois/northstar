// Copyright (c) 2020 ESRLabs
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.

use super::model;
use futures::Stream;
use std::{
    cmp::min,
    io::ErrorKind,
    pin::Pin,
    task::{self, Poll},
};
use task::Context;
use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, FramedParts};

/// Newline delimited json codec for api::Message that on top implementes AsyncRead and Write
pub struct Framed<T> {
    inner: tokio_util::codec::Framed<T, Codec>,
}

impl<T> Framed<T> {
    /// Consumes the Framed, returning its underlying I/O stream, the buffer with unprocessed data, and the codec.
    pub fn into_parts(self) -> FramedParts<T, Codec> {
        self.inner.into_parts()
    }

    /// Consumes the Framed, returning its underlying I/O stream.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

/// Constructs a new Framed with Codec from `io`
pub fn framed<T: AsyncRead + AsyncWrite>(io: T) -> Framed<T> {
    Framed {
        inner: tokio_util::codec::Framed::new(io, Codec::default()),
    }
}

/// Newline delimited json
#[derive(Default)]
pub struct Codec {
    lines: tokio_util::codec::LinesCodec,
}

impl Decoder for Codec {
    type Item = model::Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines
            .decode(src)
            .map_err(|e| io::Error::new(ErrorKind::Other, e))? // See LinesCodecError.
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }
}

impl Encoder<model::Message> for Codec {
    type Error = io::Error;

    fn encode(
        &mut self,
        item: model::Message,
        dst: &mut bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        self.lines
            .encode(serde_json::to_string(&item)?.as_str(), dst)
            .map_err(|e| io::Error::new(ErrorKind::Other, e))
    }
}

impl<T: Unpin + AsyncRead + AsyncWrite> AsyncWrite for Framed<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let t: &mut T = self.inner.get_mut();
        AsyncWrite::poll_write(Pin::new(t), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let t: &mut T = self.inner.get_mut();
        AsyncWrite::poll_flush(Pin::new(t), cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let t: &mut T = self.inner.get_mut();
        AsyncWrite::poll_shutdown(Pin::new(t), cx)
    }
}

impl<T: Unpin + AsyncRead + AsyncWrite> AsyncRead for Framed<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.inner.read_buffer().is_empty() {
            let t: &mut T = self.inner.get_mut();
            AsyncRead::poll_read(Pin::new(t), cx, buf)
        } else {
            let n = min(buf.remaining(), self.inner.read_buffer().len());
            buf.put_slice(&self.inner.read_buffer_mut().split_to(n));
            Poll::Ready(Ok(()))
        }
    }
}

impl<T: Unpin + AsyncWrite + AsyncRead> Stream for Framed<T> {
    type Item = Result<model::Message, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let framed = Pin::new(&mut self.inner);
        framed.poll_next(cx)
    }
}

impl<T: Unpin + AsyncRead + AsyncWrite> futures::sink::Sink<model::Message> for Framed<T> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: model::Message) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use proptest::{prelude::Just, proptest, strategy::Strategy};

    proptest! {
        #[test]
        fn encoding_a_message_then_decoding_it_yields_the_same_message(initial_message in mk_message()) {
            // Pre-condition.
            let mut message_as_bytes = BytesMut::default();

            // Action.
            let mut codec = Codec::default();

            codec.encode(initial_message.clone(), &mut message_as_bytes)?;
            let message = codec.decode(&mut message_as_bytes)?;

            // Post-condition.
            assert_eq!(message, Some(initial_message));
        }
    }

    fn mk_message() -> impl Strategy<Value = model::Message> {
        mk_simple_payload().prop_map(model::Message::new)
    }

    fn mk_simple_payload() -> impl Strategy<Value = model::Payload> {
        // TODO: Perhaps consider all the variants?
        Just(model::Payload::Request(model::Request::Containers))
    }
}
