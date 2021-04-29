use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Unexpected w3gs packet: {0:?}")]
  UnexpectedW3GSPacket(flo_w3gs::packet::Packet),
  #[error("Stream closed unexpectedly")]
  StreamClosed,
  #[error("player emulator: {0}")]
  PlayerEmulator(#[from] crate::player_emulator::PlayerEmulatorError),
  #[error("Lan: {0}")]
  Lan(#[from] flo_lan::error::Error),
  #[error("W3GS: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("Map: {0}")]
  War3Map(#[from] flo_w3map::error::Error),
  #[error("War3 data: {0}")]
  War3Data(#[from] flo_w3storage::error::Error),
  #[error("Net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("Io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
