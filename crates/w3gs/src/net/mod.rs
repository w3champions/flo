use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;

use crate::error::*;

mod codec;
use self::codec::W3GSCodec;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

#[derive(Debug)]
pub struct W3GSListener {
  listener: TcpListener,
  local_addr: SocketAddr,
}

impl W3GSListener {
  pub async fn bind() -> Result<Self, Error> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 6), 0)).await?;
    let local_addr = listener.local_addr()?;
    Ok(W3GSListener {
      listener,
      local_addr,
    })
  }

  pub async fn accept(&mut self) -> W3GSStream {
    loop {
      match self.listener.accept().await {
        Ok((mut stream, addr)) => {
          println!("W3GSListenner::accept: {}", addr);
          stream.set_nodelay(true);
          stream.set_keepalive(None);
          return W3GSStream {
            addr,
            inner: Framed::new(stream, W3GSCodec::new()),
          };
        }
        Err(e) => eprintln!("W3GSListenner::accept: {}", e),
      }
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
  addr: SocketAddr,
  inner: Framed<TcpStream, W3GSCodec>,
}
