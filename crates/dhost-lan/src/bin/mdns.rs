use pretty_hex::*;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::UdpSocket;

/// Bind socket to multicast address with IP_MULTICAST_LOOP and SO_REUSEADDR Enabled
fn bind_multicast(
  addr: &SocketAddrV4,
  multi_addr: &SocketAddrV4,
) -> Result<std::net::UdpSocket, Box<dyn Error>> {
  use socket2::{Domain, Protocol, Socket, Type};

  assert!(multi_addr.ip().is_multicast(), "Must be multcast address");

  let socket = Socket::new(Domain::ipv4(), Type::dgram(), Some(Protocol::udp()))?;

  socket.set_reuse_address(true)?;
  socket.bind(&socket2::SockAddr::from(*addr))?;
  socket.set_multicast_loop_v4(true)?;
  socket.join_multicast_v4(multi_addr.ip(), addr.ip())?;

  Ok(socket.into_udp_socket())
}

#[tokio::main]
async fn main() {
  let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED.into(), 5353);
  let multicast_ip = Ipv4Addr::new(224, 0, 0, 251);
  let multicast_addr = SocketAddrV4::new(multicast_ip.clone().into(), 5353);

  let std_socket = bind_multicast(&addr, &multicast_addr).expect("Failed to bind multicast socket");
  let mut socket = UdpSocket::from_std(std_socket).unwrap();
  let addr = match socket.local_addr() {
    Ok(SocketAddr::V4(v4)) => v4,
    Ok(_) => unreachable!(),
    Err(e) => panic!("get addr error: {}", e),
  };
  println!("listing on {:?}", addr);

  let mut buffer = Vec::<u8>::with_capacity(4096);
  buffer.resize(4096, 0);

  loop {
    let (len, addr) = socket.recv_from(&mut buffer).await.unwrap();
    println!("recv from {}: len = {}", addr, len);
    println!("{:?}", (&buffer[..len] as &[u8]).hex_dump());
  }
}
