use super::*;
use crate::{BinDecode, BinEncode};
pub use std::net::{Ipv4Addr, SocketAddrV4};

#[derive(BinDecode, BinEncode, PartialEq)]
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

impl std::fmt::Debug for SockAddr {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self.family {
      0 => f
        .debug_struct("SockAddr")
        .field("family", &self.family)
        .finish(),
      2 => f
        .debug_struct("SockAddr")
        .field("family", &self.family)
        .field("addr_v4", &self.addr_v4)
        .finish(),
      _ => f
        .debug_struct("SockAddr")
        .field("family", &self.family)
        .field("_unknown", &self._unknown)
        .finish(),
    }
  }
}

impl SockAddr {
  pub fn new_ipv4(ip_octets: [u8; 4], port: u16) -> Self {
    SockAddr {
      family: 2,
      addr_v4: Some(SocketAddrV4::new(Ipv4Addr::from(ip_octets), port)),
      _addr_v4_reserved: Some([0; 8]),
      _unknown: None,
    }
  }

  pub fn new_null() -> Self {
    SockAddr {
      family: 0,
      addr_v4: None,
      _addr_v4_reserved: None,
      _unknown: Some([0; 14]),
    }
  }
}

impl BinEncode for Ipv4Addr {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_slice(&self.octets())
  }
}

impl BinDecode for Ipv4Addr {
  const MIN_SIZE: usize = 4;
  const FIXED_SIZE: bool = true;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(Self::MIN_SIZE)?;
    let mut octets: [u8; 4] = [0; 4];
    buf.copy_to_slice(&mut octets);
    Ok(Ipv4Addr::from(octets))
  }
}

impl BinEncode for SocketAddrV4 {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    self.port().encode(buf);
    self.ip().encode(buf);
  }
}

impl BinDecode for SocketAddrV4 {
  const MIN_SIZE: usize = 4 + 2;
  const FIXED_SIZE: bool = true;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(Self::MIN_SIZE)?;
    let port = buf.get_u16_le();
    let ip = Ipv4Addr::decode(buf)?;
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
  let mut bytes: &[u8] = &[0x34, 0x12, 192, 168, 0, 1];
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
    0x34, 0x12, 192, 168, 0, 1, // ip addr
    0, 0, 0, 0, 0, 0, 0, 0, // reserved
  ];
  assert_eq!(SockAddr::decode(&mut bytes).unwrap(), v);

  let mut encoded: &mut [u8] = &mut [0; 2 + 6 + 8];
  v.encode(&mut encoded);
  assert_eq!(encoded, bytes);
}
