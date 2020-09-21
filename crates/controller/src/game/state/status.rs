use crate::error::*;
use crate::game::state::GameActor;
use crate::game::{db, GameStatus, NodeGameStatus, SlotClientStatus};
use crate::player::session::get_session_update_packet;
use crate::player::state::sender::PlayerFrames;
use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Context, Handler, Message};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use std::collections::HashMap;

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::PacketClientUpdateSlotClientStatus))]
pub struct GameSlotClientStatusUpdate {
  pub player_id: i32,
  pub game_id: i32,
  #[s2_grpc(proto_enum)]
  pub status: SlotClientStatus,
}

impl Message for GameSlotClientStatusUpdate {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<GameSlotClientStatusUpdate> for GameActor {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    message: GameSlotClientStatusUpdate,
  ) -> Result<()> {
    let game_id = message.game_id;
    let player_id = message.player_id;
    tracing::debug!(
      game_id,
      player_id,
      "update game slot client status: {:?}",
      message
    );

    let status = message.status;

    self
      .db
      .exec(move |conn| db::update_slot_client_status(conn, game_id, player_id, status))
      .await?;

    let mut pkt = proto::flo_connect::PacketGameSlotClientStatusUpdate {
      player_id,
      game_id,
      ..Default::default()
    };
    pkt.set_status(status.into_proto_enum());

    self
      .player_packet_sender
      .broadcast(self.players.clone(), pkt.encode_as_frame()?)
      .await?;

    self.player_client_status_map.insert(player_id, status);

    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct GameStatusUpdate {
  pub game_id: i32,
  pub status: NodeGameStatus,
  pub updated_player_game_client_status_map: HashMap<i32, SlotClientStatus>,
}

impl Message for GameStatusUpdate {
  type Result = Result<GameStatus>;
}

#[async_trait]
impl Handler<GameStatusUpdate> for GameActor {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    message: GameStatusUpdate,
  ) -> Result<GameStatus> {
    self
      .db
      .exec({
        let message = message.clone();
        move |conn| -> Result<_> {
          db::update_status(conn, &message)?;
          Ok(())
        }
      })
      .await?;

    let frame_game_status = message.to_packet().encode_as_frame()?;

    let ended = match self.status {
      GameStatus::Ended | GameStatus::Terminated => true,
      _ => false,
    };

    let frame_iter = self
      .players
      .iter()
      .map(|player_id| {
        // update player session if the player is still in game
        if ended {
          get_session_update_packet(None)
            .encode_as_frame()
            .map(|frame_session_update| {
              (
                *player_id,
                PlayerFrames::from(vec![frame_game_status.clone(), frame_session_update]),
              )
            })
            .map_err(Error::from)
        } else {
          Ok((*player_id, PlayerFrames::from(frame_game_status.clone())))
        }
      })
      .collect::<Result<Vec<_>>>()?;

    self.status = GameStatus::from(message.status);
    self
      .player_client_status_map
      .extend(message.updated_player_game_client_status_map);

    self.player_packet_sender.broadcast_map(frame_iter).await?;

    Ok(self.status)
  }
}

impl GameStatusUpdate {
  pub fn to_packet(&self) -> flo_net::proto::flo_node::PacketNodeGameStatusUpdate {
    let mut pkt = flo_net::proto::flo_node::PacketNodeGameStatusUpdate {
      game_id: self.game_id,
      ..Default::default()
    };
    pkt.set_status(self.status.into_proto_enum());
    for (id, status) in &self.updated_player_game_client_status_map {
      pkt.insert_updated_player_game_client_status_map(*id, status.into_proto_enum());
    }
    pkt
  }
}

impl From<flo_net::proto::flo_node::PacketNodeGameStatusUpdate> for GameStatusUpdate {
  fn from(pkt: flo_net::proto::flo_node::PacketNodeGameStatusUpdate) -> Self {
    GameStatusUpdate {
      game_id: pkt.game_id,
      status: NodeGameStatus::unpack_enum(pkt.status()),
      updated_player_game_client_status_map: pkt
        .updated_player_game_client_status_map
        .into_iter()
        .map(|(k, v)| {
          (
            k,
            flo_net::proto::flo_connect::SlotClientStatus::from_i32(v)
              .map(|v| SlotClientStatus::unpack_enum(v))
              .unwrap_or(SlotClientStatus::Pending),
          )
        })
        .collect(),
    }
  }
}
