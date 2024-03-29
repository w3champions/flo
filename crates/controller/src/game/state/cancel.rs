use crate::error::*;
use crate::game::state::GameActor;

use crate::player::state::sender::PlayerFrames;

use flo_net::packet::FloPacket;

use flo_state::{async_trait, Context, Handler, Message};

pub struct CancelGame {
  pub player_id: Option<i32>,
}

impl Message for CancelGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<CancelGame> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CancelGame { player_id }: CancelGame,
  ) -> Result<()> {
    let game_id = self.game_id;

    self
      .db
      .exec(move |conn| crate::game::db::cancel(conn, game_id, player_id))
      .await
      .map_err(Error::from)?;

    self
      .player_reg
      .players_leave_game(self.players.clone(), game_id)
      .await?;

    let packet_iter = self
      .players
      .iter()
      .cloned()
      .map(|player_id| {
        use flo_net::proto::flo_connect::*;
        let frame_left = PacketGamePlayerLeave {
          game_id,
          player_id,
          reason: PlayerLeaveReason::GameCancelled.into(),
        }
        .encode_as_frame()?;

        Ok((player_id, PlayerFrames::from(frame_left)))
      })
      .collect::<Result<Vec<_>>>()?
      .into_iter();

    self.player_reg.broadcast_map(packet_iter).await?;

    Ok(())
  }
}
