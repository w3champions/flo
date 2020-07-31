use futures::ready;

use futures::stream::{Stream};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::{TcpListener};



use crate::error::*;

use crate::stream::FloStream;

#[derive(Debug)]
pub struct FloListener {
  listener: TcpListener,
  local_addr: SocketAddr,
}

impl FloListener {
  pub async fn bind_v4(port: u16) -> Result<Self, Error> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).await?;
    let local_addr = listener.local_addr()?;
    Ok(FloListener {
      listener,
      local_addr,
    })
  }

  pub fn incoming(&mut self) -> Incoming {
    Incoming::new(&mut self.listener)
  }

  pub fn local_addr(&self) -> &SocketAddr {
    &self.local_addr
  }

  pub fn port(&self) -> u16 {
    self.local_addr.port()
  }
}

pub struct Incoming<'a> {
  inner: &'a mut TcpListener,
}

impl Incoming<'_> {
  pub(crate) fn new(listener: &mut TcpListener) -> Incoming<'_> {
    Incoming { inner: listener }
  }

  pub fn poll_accept(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<FloStream>> {
    let (socket, _addr) = ready!(self.inner.poll_accept(cx))?;

    socket.set_nodelay(true).ok();
    socket.set_keepalive(None).ok();

    let stream = FloStream::new(socket);

    Poll::Ready(Ok(stream))
  }
}

impl Stream for Incoming<'_> {
  type Item = Result<FloStream>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let stream = ready!(self.poll_accept(cx))?;
    Poll::Ready(Some(Ok(stream)))
  }
}
