use crate::error::*;
use crate::game::state::GameActor;

use flo_state::{async_trait, Context, Handler, Message};

pub struct GetGamePlayers;

impl Message for GetGamePlayers {
  type Result = Result<Vec<i32>>;
}

#[async_trait]
impl Handler<GetGamePlayers> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetGamePlayers,
  ) -> Result<Vec<i32>> {
    Ok(self.players.clone())
  }
}

pub struct GetGamePlayersIfNodeSelected {
  pub node_ids: Vec<i32>,
}

impl Message for GetGamePlayersIfNodeSelected {
  type Result = Option<Vec<i32>>;
}

#[async_trait]
impl Handler<GetGamePlayersIfNodeSelected> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetGamePlayersIfNodeSelected { node_ids }: GetGamePlayersIfNodeSelected,
  ) -> Option<Vec<i32>> {
    if self
      .selected_node_id
      .as_ref()
      .map(|node_id| node_ids.contains(node_id))
      .unwrap_or(false)
    {
      Some(self.players.clone())
    } else {
      None
    }
  }
}
