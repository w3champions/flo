use crate::error::Result;
use tokio::net::UdpSocket;

const ECHO_DATAGRAM_LEN: usize = 4;

use flo_constants::NODE_ECHO_PORT;

pub async fn serve_echo() -> Result<()> {
  let mut socket = UdpSocket::bind(("0.0.0.0", NODE_ECHO_PORT)).await?;

  let mut recv_buf = [0_u8; ECHO_DATAGRAM_LEN];

  loop {
    if let Some((size, peer)) = socket.recv_from(&mut recv_buf).await.ok() {
      if size != ECHO_DATAGRAM_LEN {
        continue;
      }
      socket.send_to(&recv_buf[..], &peer).await.ok();
    }
  }
}
