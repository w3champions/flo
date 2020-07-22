mod error;
#[macro_use]
mod packet;
mod codec;
mod constants;
mod keep_alive;
mod listener;
mod stream;

pub mod proto {
  pub mod flo_common {
    include!(concat!(env!("OUT_DIR"), "/flo_common.rs"));
  }

  pub mod flo_connect {
    include!(concat!(env!("OUT_DIR"), "/flo_connect.rs"));
  }
}

pub mod connect;
