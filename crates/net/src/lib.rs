pub mod connect;
pub mod packets;

pub mod proto {
  pub mod flo_common {
    include!(concat!(env!("OUT_DIR"), "/flo_common.rs"));
  }

  pub mod flo_connect {
    include!(concat!(env!("OUT_DIR"), "/flo_connect.rs"));
  }
}
