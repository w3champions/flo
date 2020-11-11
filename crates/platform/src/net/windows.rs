use super::IpInfo;
use crate::error::Result;

pub fn get_ip_info() -> Result<IpInfo> {
  let mut info = IpInfo {
    ips_v4: vec![],
    ips_v6: vec![],
  };

  for adapter in ipconfig::get_adapters()? {
    if adapter.oper_status() == ipconfig::OperStatus::IfOperStatusUp
      && adapter.if_type() != ipconfig::IfType::SoftwareLoopback
    {
      for ip in adapter.ip_addresses() {
        use std::net::IpAddr;
        match ip {
          IpAddr::V4(ref ip) => info.ips_v4.push(ip.clone()),
          IpAddr::V6(ref ip) => info.ips_v6.push(ip.clone()),
        }
      }
    }
  }

  Ok(info)
}