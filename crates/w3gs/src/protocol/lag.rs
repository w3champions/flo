use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::PacketTypeId;
use crate::protocol::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct StartLag {
  _num_of_players: u8,
  #[bin(repeat = "_num_of_players")]
  players: Vec<LagPlayer>,
}

impl StartLag {
  pub fn new(players: Vec<LagPlayer>) -> Self {
    StartLag {
      _num_of_players: players.len() as u8,
      players,
    }
  }

  pub fn players(&self) -> &[LagPlayer] {
    self.players.as_ref()
  }
}

impl PacketPayload for StartLag {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::StartLag;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct LagPlayer {
  pub player_id: u8,
  pub lag_duration_ms: u32,
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct StopLag(LagPlayer);

impl PacketPayload for StopLag {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::StopLag;
}
