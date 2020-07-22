use futures::ready;
use futures::sink::SinkExt;
use futures::stream::TryStreamExt;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use crate::codec::FloFrameCodec;
use crate::error::*;
use crate::packet::{FloPacket, Frame};

#[derive(Debug)]
pub struct FloStream {
  pub(crate) peer_addr: SocketAddr,
  pub(crate) transport: Framed<TcpStream, FloFrameCodec>,
}

impl FloStream {
  pub fn peer_addr(&self) -> SocketAddr {
    self.peer_addr
  }

  pub async fn send<T>(&mut self, packet: T) -> Result<()>
  where
    T: FloPacket,
  {
    self.transport.send(packet.encode_as_frame()?).await?;
    Ok(())
  }

  pub async fn send_all<I, T>(&mut self, iter: I) -> Result<()>
  where
    I: IntoIterator<Item = T>,
    T: FloPacket,
  {
    let mut stream = tokio::stream::iter(iter.into_iter().map(|p| p.encode_as_frame()));
    self.transport.send_all(&mut stream).await?;
    Ok(())
  }

  pub async fn recv<T>(&mut self) -> Result<T>
  where
    T: FloPacket + Default,
  {
    let frame = self
      .transport
      .try_next()
      .await?
      .ok_or_else(|| Error::StreamClosed)?;
    Ok(frame.decode()?)
  }
}

#[test]
fn test_lookup() {
  use std::net::{SocketAddr, ToSocketAddrs};
  let mut addrs_iter = "wc3.tools:443".to_socket_addrs().unwrap();
  dbg!(addrs_iter.next());
}
