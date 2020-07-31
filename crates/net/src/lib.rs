mod codec;
mod common;
mod version;

pub mod error;
#[macro_use]
pub mod packet;

pub mod constants;
pub mod listener;
pub mod stream;
pub mod time;

pub mod proto {
  pub mod flo_common {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};
    include!(concat!(env!("OUT_DIR"), "/flo_common.rs"));
  }

  pub mod flo_connect {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};
    include!(concat!(env!("OUT_DIR"), "/flo_connect.rs"));
  }
}

pub mod connect;
