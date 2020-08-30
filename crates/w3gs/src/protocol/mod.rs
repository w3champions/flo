pub mod action;
pub mod chat;
pub mod constants;
pub mod desync;
pub mod game;
pub mod join;
pub mod lag;
pub mod leave;
pub mod map;
pub mod packet;
pub mod ping;
pub mod player;
pub mod slot;

mod protobuf {
  include!(concat!(env!("OUT_DIR"), "/w3gs.rs"));
}
