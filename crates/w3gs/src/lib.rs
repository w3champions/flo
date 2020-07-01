pub mod constants;
pub mod error;
pub mod game;
pub mod join;
pub mod map;
pub mod net;
pub mod packet;
pub mod ping;
pub mod player;
pub mod slot;

mod proto {
  include!(concat!(env!("OUT_DIR"), "/w3gs.rs"));
}
