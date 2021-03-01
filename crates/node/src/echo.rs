use crate::error::Result;
use std::net::{Ipv4Addr, SocketAddrV4};
use tokio::net::UdpSocket;
const ALLOWED_ECHO_DATAGRAM_LEN: &[usize] = &[4, 8];
const MAX_RECV_BUF: usize = 8;

use flo_constants::NODE_ECHO_PORT;

pub async fn serve_echo() -> Result<()> {
  let mut socket =
    UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, NODE_ECHO_PORT)).await?;

  let mut recv_buf = [0_u8; MAX_RECV_BUF];

  loop {
    if let Some((size, peer)) = socket.recv_from(&mut recv_buf).await.ok() {
      if !ALLOWED_ECHO_DATAGRAM_LEN.contains(&size) {
        continue;
      }
      socket.send_to(&recv_buf[..size], &peer).await.ok();
    }
  }
}
