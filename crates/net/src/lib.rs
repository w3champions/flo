mod codec;
mod common;
mod version;

pub mod error;
#[macro_use]
pub mod packet;

pub mod constants;
pub mod listener;
pub mod ping;
pub mod stream;
pub mod time;
pub mod w3gs;

pub mod proto {
  pub mod flo_common {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};
    include!(concat!(env!("OUT_DIR"), "/flo_common.rs"));
  }

  pub mod flo_connect {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};

    pub use super::flo_common::{Computer, Race, SlotClientStatus, SlotSettings, SlotStatus};
    pub use super::flo_node::PacketClientUpdateSlotClientStatus;

    include!(concat!(env!("OUT_DIR"), "/flo_connect.rs"));
  }

  pub mod flo_node {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};

    pub use super::flo_common::{Computer, Race, SlotClientStatus, SlotSettings, SlotStatus};

    include!(concat!(env!("OUT_DIR"), "/flo_node.rs"));
  }

  pub mod flo_observer {
    #[allow(unused)]
    use serde::{Deserialize, Serialize};

    pub use super::flo_common::{SlotSettings, Version};

    include!(concat!(env!("OUT_DIR"), "/flo_observer.rs"));
  }
}

pub mod connect;
pub mod node;
pub mod observer;
