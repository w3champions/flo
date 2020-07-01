pub mod mdns;

pub mod error;
mod game_info;

pub use self::game_info::GameInfo;

pub mod proto {
  include!(concat!(env!("OUT_DIR"), "/wc3.rs"));
}
