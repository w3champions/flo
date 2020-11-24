use futures::sink::SinkExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::time::timeout;
use tokio_util::codec::Framed;

use crate::codec::FloFrameCodec;
use crate::error::*;
use crate::packet::{FloPacket, Frame};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct FloStream {
  pub timeout: Duration,
  pub(crate) transport: Framed<TcpStream, FloFrameCodec>,
}

impl FloStream {
  pub async fn connect_no_delay<A: ToSocketAddrs>(addr: A) -> Result<Self> {
    let socket = TcpStream::connect(addr).await?;

    socket.set_nodelay(true).ok();
    socket.set_keepalive(None).ok();

    let transport = Framed::new(socket, FloFrameCodec::new());
    Ok(FloStream {
      transport,
      timeout: DEFAULT_TIMEOUT,
    })
  }

  pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
    let socket = TcpStream::connect(addr).await?;

    socket.set_keepalive(Some(Duration::from_secs(30)))?;

    let transport = Framed::new(socket, FloFrameCodec::new());
    Ok(FloStream {
      transport,
      timeout: DEFAULT_TIMEOUT,
    })
  }

  pub fn new(socket: TcpStream) -> Self {
    FloStream {
      transport: Framed::new(socket, FloFrameCodec::new()),
      timeout: DEFAULT_TIMEOUT,
    }
  }

  pub fn set_timeout(&mut self, duration: Duration) -> &mut Self {
    self.timeout = duration;
    self
  }

  #[inline]
  pub fn local_addr(&self) -> Result<SocketAddr> {
    self.transport.get_ref().local_addr().map_err(Into::into)
  }

  #[inline]
  pub fn peer_addr(&self) -> Result<SocketAddr> {
    self.transport.get_ref().peer_addr().map_err(Into::into)
  }

  pub async fn send_frame(&mut self, frame: Frame) -> Result<()> {
    timeout(self.timeout, self.transport.send(frame))
      .await
      .map_err(|_elapsed| Error::StreamTimeout)??;
    Ok(())
  }

  #[inline]
  pub async fn send_frames<I>(&mut self, iter: I) -> Result<()>
  where
    I: IntoIterator<Item = Frame>,
  {
    let mut stream = tokio::stream::iter(iter.into_iter().map(Ok));
    timeout(self.timeout, self.transport.send_all(&mut stream))
      .await
      .map_err(|_elapsed| Error::StreamTimeout)??;
    Ok(())
  }

  #[inline]
  pub async fn send<T>(&mut self, packet: T) -> Result<()>
  where
    T: FloPacket,
  {
    self.send_frame(packet.encode_as_frame()?).await?;
    Ok(())
  }

  #[inline]
  pub async fn recv<T>(&mut self) -> Result<T>
  where
    T: FloPacket + Default,
  {
    let frame = self.recv_frame().await?;
    Ok(frame.decode()?)
  }

  #[inline]
  pub async fn recv_timeout<T>(&mut self, duration: Duration) -> Result<T>
  where
    T: FloPacket + Default,
  {
    let frame = timeout(duration, self.recv_frame())
      .await
      .map_err(|_elapsed| Error::StreamTimeout)??;
    Ok(frame.decode()?)
  }

  #[inline]
  pub async fn recv_frame(&mut self) -> Result<Frame> {
    let frame = self
      .transport
      .try_next()
      .await?
      .ok_or_else(|| Error::StreamClosed)?;
    Ok(frame)
  }

  #[inline]
  pub async fn recv_frame_timeout(&mut self) -> Result<Frame> {
    let frame = timeout(self.timeout, self.transport.try_next())
      .await
      .map_err(|_elapsed| Error::StreamTimeout)??
      .ok_or_else(|| Error::StreamClosed)?;
    Ok(frame)
  }

  pub async fn flush(&mut self) -> Result<()> {
    self.transport.flush().await?;
    Ok(())
  }
}

impl Stream for FloStream {
  type Item = Result<Frame>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Pin::new(&mut self.transport).poll_next(cx)
  }
}

#[test]
fn test_lookup() {
  use std::net::ToSocketAddrs;
  let mut addrs_iter = "wc3.tools:443".to_socket_addrs().unwrap();
  dbg!(addrs_iter.next());
}
