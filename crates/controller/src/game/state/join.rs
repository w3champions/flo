use crate::error::*;
use crate::game::state::GameActor;
use crate::game::Game;
use diesel::prelude::*;
use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Context, Handler, Message};

use s2_grpc_utils::S2ProtoPack;

pub struct PlayerJoin {
  pub player_id: i32,
}

impl Message for PlayerJoin {
  type Result = Result<Game>;
}

#[async_trait]
impl Handler<PlayerJoin> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    PlayerJoin { player_id }: PlayerJoin,
  ) -> Result<Game> {
    let game_id = self.game_id;
    let (game, mute_list) = self
      .db
      .exec(move |conn| {
        conn.transaction(|| {
          crate::game::db::add_player(conn, game_id, player_id)?;
          let game = crate::game::db::get_full(conn, game_id)?;
          let mut mute_list_map =
            crate::player::db::get_mute_list_map(conn, &game.get_player_ids())?;
          Ok::<_, Error>((game, mute_list_map.remove(&player_id).unwrap_or_default()))
        })
      })
      .await?;

    self.players.push(player_id);

    // send game info to joined player
    self
      .player_reg
      .player_replace_game(player_id, game.clone(), mute_list)
      .await?;

    {
      let slot_info = game
        .get_player_slot_info(player_id)
        .ok_or_else(|| Error::PlayerSlotNotFound)?;
      let player: proto::flo_connect::PlayerInfo = slot_info.player.clone().pack()?;

      // send notification to other players in this game
      let mut players = game.get_player_ids();
      players.retain(|id| *id != player_id);
      let frame = {
        use proto::flo_connect::*;
        PacketGamePlayerEnter {
          game_id: game.id,
          slot_index: slot_info.slot_index as i32,
          slot: Slot {
            player: Some(player),
            settings: Some(slot_info.slot.settings.clone().pack()?),
            ..Default::default()
          }
          .into(),
        }
      }
      .encode_as_frame()?;
      self.player_reg.broadcast(players, frame).await?;
    }

    Ok(game)
  }
}
