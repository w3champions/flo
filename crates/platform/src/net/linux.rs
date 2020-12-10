use super::IpInfo;
use crate::error::{Error, Result};
use std::net::IpAddr;

pub fn get_ip_info() -> Result<IpInfo> {
  let mut info = IpInfo {
    ips_v4: vec![],
    ips_v6: vec![],
  };

  for i in get_if_addrs::get_if_addrs().map_err(Error::GetIfAddrs)? {
    if !i.is_loopback() {
      match i.ip() {
        IpAddr::V4(ref ip) => info.ips_v4.push(ip.clone()),
        IpAddr::V6(ref ip) => info.ips_v6.push(ip.clone()),
      }
    }
  }

  Ok(info)
}

#[test]
fn test_get_if_addrs() {
  dbg!(get_ip_info().unwrap());
}
