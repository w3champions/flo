use crate::error::*;
use crate::game::state::GameActor;

use flo_state::{async_trait, Context, Handler, Message};

pub struct GetGamePlayers;

impl Message for GetGamePlayers {
  type Result = Result<Vec<i32>>;
}

#[async_trait]
impl Handler<GetGamePlayers> for GameActor {
  async fn handle(&mut self, _: &mut Context<Self>, _: GetGamePlayers) -> Result<Vec<i32>> {
    Ok(self.players.clone())
  }
}
