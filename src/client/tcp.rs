//Copyright (c) 2017 Daniel Abramov
//Copyright (c) 2017 Alexey Galakhov
//
//Permission is hereby granted, free of charge, to any person obtaining a copy
//of this software and associated documentation files (the "Software"), to deal
//in the Software without restriction, including without limitation the rights
//to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//copies of the Software, and to permit persons to whom the Software is
//furnished to do so, subject to the following conditions:
//
//The above copyright notice and this permission notice shall be included in
//all copies or substantial portions of the Software.
//
//THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//THE SOFTWARE.
// -----------------------------------------------------------------------------
// The code below was adapted from the tokio-tungstenite crate:
// https://github.com/snapview/tokio-tungstenite
// It has been updated to use tokio instead of tokio-core, and with some slight
// modifications for this project.  MIT License information above as per
// licensing requirements.
// -----------------------------------------------------------------------------

use std::io::{ErrorKind as ioErrorKind};
use std::net::SocketAddr;

use futures::{Poll, Future, Async, AsyncSink, Stream, Sink, StartSend};
use tokio::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};

use tungstenite::{Error as WsError};
use tungstenite::handshake::client::{ClientHandshake, Response, Request};
use tungstenite::handshake::{HandshakeRole, HandshakeError};
use tungstenite::protocol::{WebSocket, Message};
use url::Url;

use error::{Error, ErrorKind};

pub fn connect_async(host: &str)
    -> Box<Future<Item=(WebSocketStream<TcpStream>, Response), Error=Error>>
{
    let host = host.replace("ws://", "");
    let conn = TcpStream::connect(&host.parse::<SocketAddr>().unwrap())
        .map_err(Into::into);
    let request = Request {
        url: Url::parse(&format!("ws://{}", host)).unwrap(),
        extra_headers: None
    };

    let fut = conn.and_then(|stream| {
        client_async(request, stream)
    });

    Box::new(fut)
}


pub fn client_async<'a, R, S>(request: R, stream: S) -> ConnectAsync<S>
    where
        R: Into<Request<'a>>,
        S: AsyncRead + AsyncWrite
{
    ConnectAsync {
        inner: MidHandshake {
            inner: Some(ClientHandshake::start(stream, request.into()).handshake())
        }
    }
}

pub struct WebSocketStream<S> {
    inner: WebSocket<S>,
}

impl<T> Stream for WebSocketStream<T> where T: AsyncRead + AsyncWrite {
    type Item = Message;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Message>, Error> {
        self.inner.read_message().map(|m| Some(m)).to_async().map_err(Into::into)
    }
}

impl<T> Sink for WebSocketStream<T> where T: AsyncRead + AsyncWrite {
    type SinkItem = Message;
    type SinkError = Error;

    fn start_send(&mut self, item: Message) -> StartSend<Message, Error> {
        self.inner.write_message(item).to_async()?;
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Error> {
        self.inner.write_pending().to_async().map_err(Into::into)
    }

    fn close(&mut self) -> Poll<(), Error> {
        self.inner.close(None).to_async().map_err(Into::into)
    }
}

pub struct ConnectAsync<S: AsyncRead + AsyncWrite> {
    inner: MidHandshake<ClientHandshake<S>>,
}

impl<S: AsyncRead + AsyncWrite> Future for ConnectAsync<S> {
    type Item = (WebSocketStream<S>, Response);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Error> {
        match self.inner.poll()? {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready((ws, resp)) => Ok(Async::Ready((WebSocketStream { inner: ws }, resp))),
        }
    }
}

struct MidHandshake<H: HandshakeRole> {
    inner: Option<Result<<H as HandshakeRole>::FinalResult, HandshakeError<H>>>,
}

impl<H: HandshakeRole> Future for MidHandshake<H> {
    type Item = <H as HandshakeRole>::FinalResult;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Error> {
        match self.inner.take().expect("Cannot poll MidHandshake twice") {
            Ok(result) => Ok(Async::Ready(result)),
            Err(HandshakeError::Failure(e)) => Err(ErrorKind::Ws(e).into()),
            Err(HandshakeError::Interrupted(s)) => {
                match s.handshake() {
                    Ok(result) => Ok(Async::Ready(result)),
                    Err(HandshakeError::Failure(e)) => Err(ErrorKind::Ws(e).into()),
                    Err(HandshakeError::Interrupted(s)) => {
                        self.inner = Some(Err(HandshakeError::Interrupted(s)));
                        Ok(Async::NotReady)
                    }
                }
            }
        }
    }
}

trait ToAsync {
    type T;
    type E;
    fn to_async(self) -> Result<Async<Self::T>, Self::E>;
}

impl<T> ToAsync for Result<T, WsError> {
    type T = T;
    type E = WsError;
    fn to_async(self) -> Result<Async<Self::T>, Self::E> {
        match self {
            Ok(x) => Ok(Async::Ready(x)),
            Err(error) => match error {
                WsError::Io(ref err) if err.kind() == ioErrorKind::WouldBlock => Ok(Async::NotReady),
                err => Err(err)
            },
        }
    }
}

