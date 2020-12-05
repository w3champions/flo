use std::net::{Ipv4Addr, Ipv6Addr};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[derive(Debug)]
pub struct IpInfo {
  pub ips_v4: Vec<Ipv4Addr>,
  pub ips_v6: Vec<Ipv6Addr>,
}
