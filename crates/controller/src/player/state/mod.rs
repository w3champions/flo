pub mod conn;
pub mod ping;
pub mod sender;

use crate::client::PlayerSender;
use crate::error::Error;
use crate::state::Data;
use flo_state::{async_trait, Actor, RegistryRef, Service};
use flo_types::ping::PingStats;

use crate::player::state::sender::PlayerFrames;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct PlayerRegistry {
  registry: BTreeMap<i32, PlayerState>,
}

impl PlayerRegistry {
  pub fn new() -> Self {
    Self {
      registry: Default::default(),
    }
  }
}

impl Actor for PlayerRegistry {}

#[async_trait]
impl Service<Data> for PlayerRegistry {
  type Error = Error;

  async fn create(_registry: &mut RegistryRef<Data>) -> Result<Self, Self::Error> {
    Ok(PlayerRegistry::new())
  }
}

#[derive(Debug)]
pub struct PlayerState {
  pub player_id: i32,
  pub ping_map: BTreeMap<i32, PingStats>,
  pub game_id: Option<i32>,
  pub sender: PlayerSender,
}

impl PlayerState {
  fn new(player_id: i32, game_id: Option<i32>, sender: PlayerSender) -> PlayerState {
    Self {
      player_id,
      game_id,
      ping_map: Default::default(),
      sender,
    }
  }

  fn try_send_frames(&mut self, frames: PlayerFrames) -> bool {
    for frame in frames {
      if !self.sender.try_send(frame) {
        return false;
      }
    }
    true
  }

  async fn shutdown(mut self) {
    self.sender.disconnect_multi().await;
  }
}
