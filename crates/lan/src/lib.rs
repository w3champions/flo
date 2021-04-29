mod game_info;
mod mdns;
mod proto {
  include!(concat!(env!("OUT_DIR"), "/wc3.rs"));
}

pub mod error;

pub use self::game_info::GameInfo;
pub use self::mdns::publisher::MdnsPublisher;
pub use self::mdns::search::{search_lan_games, LanGame};
