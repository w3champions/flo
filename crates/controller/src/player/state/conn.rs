use super::PlayerRegistry;
use crate::client::PlayerSender;
use crate::player::state::PlayerState;
use flo_state::{async_trait, Context, Handler, Message};

pub struct Connect {
  pub sender: PlayerSender,
}

impl Message for Connect {
  type Result = ();
}

#[async_trait]
impl Handler<Connect> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, message: Connect) {
    let player_id = message.sender.player_id();
    let removed = self
      .registry
      .insert(player_id, PlayerState::new(player_id, message.sender));
    if let Some(state) = removed {
      state.shutdown().await;
    }
  }
}

pub struct Disconnect {
  pub player_id: i32,
}

impl Message for Disconnect {
  type Result = ();
}

#[async_trait]
impl Handler<Disconnect> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, message: Disconnect) {
    let player_id = message.player_id;
    if let Some(state) = self.registry.remove(&player_id) {
      state.shutdown().await;
    }
  }
}
