use futures::sink::SinkExt;
use futures::stream::TryStreamExt;
use futures::{ready, StreamExt};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio_stream::Stream;
use tokio_util::codec::Framed;

use crate::error::*;
use crate::protocol::packet::{Packet, PacketPayload, PacketPayloadDecode};

mod codec;
use self::codec::W3GSCodec;

#[derive(Debug)]
pub struct W3GSListener {
  listener: TcpListener,
  local_addr: SocketAddr,
}

impl W3GSListener {
  pub async fn bind() -> Result<Self, Error> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
    let local_addr = listener.local_addr()?;
    Ok(W3GSListener {
      listener,
      local_addr,
    })
  }

  pub fn incoming(&mut self) -> Incoming {
    Incoming::new(&mut self.listener)
  }

  pub async fn accept(&mut self) -> Result<Option<W3GSStream>> {
    match Incoming::new(&mut self.listener).next().await {
      None => Ok(None),
      Some(res) => Ok(Some(res?)),
    }
  }

  pub fn local_addr(&self) -> &SocketAddr {
    &self.local_addr
  }

  pub fn port(&self) -> u16 {
    self.local_addr.port()
  }
}

#[derive(Debug)]
pub struct W3GSStream {
  local_addr: SocketAddr,
  peer_addr: Option<SocketAddr>,
  transport: Framed<TcpStream, W3GSCodec>,
}

impl W3GSStream {
  pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
    let socket = TcpStream::connect(addr).await?;
    Ok(W3GSStream {
      local_addr: socket.local_addr()?,
      peer_addr: None,
      transport: Framed::new(socket, W3GSCodec::new()),
    })
  }

  pub fn local_addr(&self) -> SocketAddr {
    self.local_addr
  }
  pub fn peer_addr(&self) -> Option<SocketAddr> {
    self.peer_addr
  }

  #[inline]
  pub async fn send(&mut self, packet: Packet) -> Result<()> {
    self.transport.send(packet).await?;
    Ok(())
  }

  #[inline]
  pub async fn send_all<I>(&mut self, iter: I) -> Result<()>
  where
    I: IntoIterator<Item = Packet>,
  {
    let mut stream = tokio_stream::iter(iter.into_iter().map(Ok));
    self.transport.send_all(&mut stream).await?;
    Ok(())
  }

  #[inline]
  pub async fn recv(&mut self) -> Result<Option<Packet>> {
    let packet = self.transport.try_next().await?;
    Ok(packet)
  }

  #[inline]
  pub async fn recv_decode<T>(&mut self) -> Result<T>
  where
    T: PacketPayloadDecode + PacketPayload,
  {
    let pkt = self.recv().await?.ok_or_else(|| Error::StreamClosed)?;

    if pkt.type_id() != T::PACKET_TYPE_ID {
      return Err(Error::PacketTypeIdMismatch {
        expected: T::PACKET_TYPE_ID,
        found: pkt.type_id(),
      });
    }

    Ok(pkt.decode_payload()?)
  }

  #[inline]
  pub async fn flush(&mut self) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    self.transport.get_mut().flush().await?;
    Ok(())
  }
}

pub struct Incoming<'a> {
  inner: &'a mut TcpListener,
}

impl Incoming<'_> {
  pub(crate) fn new(listener: &mut TcpListener) -> Incoming<'_> {
    Incoming { inner: listener }
  }

  #[inline]
  pub fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<W3GSStream>> {
    let (socket, addr) = ready!(self.inner.poll_accept(cx))?;

    socket.set_nodelay(true).ok();

    //TODO: for now not supported by tokio https://github.com/tokio-rs/tokio/pull/3146
    //socket.set_keepalive(None).ok();

    let stream = W3GSStream {
      local_addr: socket.local_addr()?,
      peer_addr: Some(addr),
      transport: Framed::new(socket, W3GSCodec::new()),
    };

    Poll::Ready(Ok(stream))
  }
}

impl Stream for Incoming<'_> {
  type Item = Result<W3GSStream>;

  #[inline]
  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let stream = ready!(self.poll_accept(cx))?;
    Poll::Ready(Some(Ok(stream)))
  }
}
