use super::*;
use crate::{BinDecode, BinEncode};
pub use std::net::{Ipv4Addr, SocketAddrV4};

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
#[bin(mod_path = "super")]
pub struct SockAddr {
  pub family: u16,

  #[bin(condition = "family == 2")]
  pub addr_v4: Option<SocketAddrV4>,
  #[bin(eq = &[0; 8])]
  #[bin(condition = "family == 2")]
  _addr_v4_reserved: Option<[u8; 8]>,

  #[bin(condition = "family != 2")]
  _unknown: Option<[u8; 14]>,
}

impl BinEncode for Ipv4Addr {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_slice(&self.octets());
  }
}

impl BinDecode for Ipv4Addr {
  const MIN_SIZE: usize = 4;
  const FIXED_SIZE: bool = true;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(Self::MIN_SIZE)?;
    let mut octets: [u8; 4] = [0; 4];
    buf.copy_to_slice(&mut octets);
    Ok(Self::from(octets))
  }
}

impl BinEncode for SocketAddrV4 {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    self.ip().encode(buf);
    self.port().encode(buf);
  }
}

impl BinDecode for SocketAddrV4 {
  const MIN_SIZE: usize = 4 + 2;
  const FIXED_SIZE: bool = true;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(Self::MIN_SIZE)?;
    let ip = Ipv4Addr::decode(buf)?;
    let port = buf.get_u16_le();
    Ok(Self::new(ip, port))
  }
}

#[test]
fn test_ip_v4_addr() {
  use super::*;
  let v = Ipv4Addr::new(192, 168, 0, 1);
  let mut bytes: &[u8] = &[192, 168, 0, 1];
  assert_eq!(Ipv4Addr::decode(&mut bytes).unwrap(), v);

  let mut encoded: &mut [u8] = &mut [0; 4];
  v.encode(&mut encoded);
  assert_eq!(encoded, bytes);
}

#[test]
fn test_sock_addr_v4() {
  use super::*;
  let v = SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 0x1234);
  let mut bytes: &[u8] = &[192, 168, 0, 1, 0x34, 0x12];
  assert_eq!(SocketAddrV4::decode(&mut bytes).unwrap(), v);

  let mut encoded: &mut [u8] = &mut [0; 6];
  v.encode(&mut encoded);
  assert_eq!(encoded, bytes);
}

#[test]
fn test_sock_addr() {
  use super::*;
  let v = SockAddr {
    family: 2,
    addr_v4: Some(SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 0x1234)),
    _addr_v4_reserved: Some([0; 8]),
    _unknown: None,
  };
  let mut bytes: &[u8] = &[
    2, 0, // family
    192, 168, 0, 1, 0x34, 0x12, // ip addr
    0, 0, 0, 0, 0, 0, 0, 0, // reserved
  ];
  assert_eq!(SockAddr::decode(&mut bytes).unwrap(), v);

  let mut encoded: &mut [u8] = &mut [0; 2 + 6 + 8];
  v.encode(&mut encoded);
  assert_eq!(encoded, bytes);
}
