use crate::error::*;
use crate::game::state::GameActor;
use crate::game::{GameStatus, SlotClientStatus};
use crate::node::messages::NodeCreateGame;

use crate::player::state::sender::PlayerFrames;
use crate::state::ActorMapExt;
use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};
use std::collections::HashMap;
use std::time::Duration;

use tokio::time::delay_for;

const TIMEOUT: Duration = Duration::from_secs(5);

pub struct StartGameCheck {
  pub player_id: i32,
}

impl Message for StartGameCheck {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartGameCheck> for GameActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    StartGameCheck { player_id }: StartGameCheck,
  ) -> Result<()> {
    let game_id = self.game_id;

    if self.host_player != player_id {
      return Err(Error::PlayerNotHost);
    }

    if self.selected_node_id.is_none() {
      return Err(Error::GameNodeNotSelected);
    }

    let players = self.players.clone();
    if self.start_state.is_some() {
      return Err(Error::GameStarted);
    }

    self.start_state = StartGameState::new(game_id, ctx.addr(), players)
      .start()
      .into();

    let frame = proto::flo_connect::PacketGameStarting { game_id }.encode_as_frame()?;
    self
      .player_packet_sender
      .broadcast(self.players.clone(), frame)
      .await?;

    Ok(())
  }
}

pub struct StartGameCheckProceed {
  pub map: HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>,
}

impl GameActor {
  async fn start_game_proceed(
    &mut self,
    StartGameCheckProceed { map }: StartGameCheckProceed,
  ) -> Result<()> {
    let game_id = self.game_id;

    tracing::debug!(game_id, "start game check proceed.");

    let mut pass = true;
    {
      let mut version: Option<&str> = None;
      let mut sha1: Option<&[u8]> = None;
      for req in map.values() {
        if version.get_or_insert(&req.war3_version) != &req.war3_version {
          pass = false;
          break;
        }
        if sha1.get_or_insert(&req.map_sha1).as_ref() != &req.map_sha1 as &[u8] {
          pass = false;
          break;
        }
      }
    }

    if !pass {
      self.start_state.take();
      let frame = proto::flo_connect::PacketGameStartReject {
        game_id,
        message: "Unable to start the game because the game and map version check failed."
          .to_string(),
        player_client_info_map: map,
      }
      .encode_as_frame()?;
      self
        .player_packet_sender
        .broadcast(self.players.clone(), frame)
        .await?;
      return Ok(());
    }

    let game = self
      .db
      .exec(move |conn| crate::game::db::get_full(conn, game_id))
      .await?;

    let node_id = if let Some(id) = game.node.as_ref().map(|node| node.id) {
      id
    } else {
      return Err(Error::GameNodeNotSelected);
    };

    let created = self
      .nodes
      .send_to(node_id, NodeCreateGame { game })
      .await?
      .await
      .or_cancelled();

    let created = match created {
      Ok(created) => created,
      // failed, reply host player
      Err(err) => {
        let pkt = match err {
          Error::NodeRequestTimeout => proto::flo_connect::PacketGameStartReject {
            game_id,
            message: format!("Create game timeout."),
            ..Default::default()
          },
          Error::GameCreateReject(reason) => {
            use proto::flo_node::ControllerCreateGameRejectReason;
            proto::flo_connect::PacketGameStartReject {
              game_id,
              message: match reason {
                ControllerCreateGameRejectReason::Unknown => {
                  format!("Create game request rejected.")
                }
                ControllerCreateGameRejectReason::GameExists => format!("Game already started."),
                ControllerCreateGameRejectReason::PlayerBusy => {
                  format!("Create game request rejected: Player busy.")
                }
                ControllerCreateGameRejectReason::Maintenance => {
                  format!("Create game request rejected: Server Maintenance.")
                }
              },
              ..Default::default()
            }
          }
          err => {
            tracing::error!("node create game: {}", err);
            proto::flo_connect::PacketGameStartReject {
              game_id,
              message: format!("Internal error."),
              ..Default::default()
            }
          }
        };

        self
          .player_packet_sender
          .send(self.host_player, pkt.encode_as_frame()?)
          .await?;
        return Ok(());
      }
    };

    self.player_client_status_map = self
      .players
      .iter()
      .map(|player_id| (*player_id, SlotClientStatus::Pending))
      .collect();

    let token_map = created
      .player_tokens
      .into_iter()
      .map(|token| (token.player_id, token))
      .collect::<HashMap<_, _>>();

    let packet_iter = self
      .players
      .iter()
      .filter_map(|player_id| {
        let pkt = if let Some(token) = token_map.get(player_id) {
          Some(proto::flo_connect::PacketGamePlayerToken {
            node_id,
            game_id,
            player_id: *player_id,
            player_token: token.to_vec(),
          })
        } else {
          tracing::error!(game_id, player_id, "player token was not found");
          None
        }?;
        Some((*player_id, pkt))
      })
      .map(|(player_id, pkt)| Ok((player_id, PlayerFrames::from(pkt.encode_as_frame()?))))
      .collect::<Result<Vec<_>>>()?;

    self.player_tokens = token_map
      .iter()
      .map(|(player_id, token)| (*player_id, token.bytes))
      .collect();
    self.player_packet_sender.broadcast_map(packet_iter).await?;

    self
      .db
      .exec(move |conn| crate::game::db::update_created(conn, game_id, token_map))
      .await?;
    self.status = GameStatus::Created;

    Ok(())
  }
}

pub struct StartGameCheckTimeout {
  pub map: HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>,
}

impl Message for StartGameCheckTimeout {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartGameCheckTimeout> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    StartGameCheckTimeout { map }: StartGameCheckTimeout,
  ) -> Result<()> {
    let game_id = self.game_id;

    self.start_state.take();

    let frame = proto::flo_connect::PacketGameStartReject {
      game_id,
      message: "Some of the players didn't response in time.".to_string(),
      player_client_info_map: map,
    }
    .encode_as_frame()?;
    self
      .player_packet_sender
      .broadcast(self.players.clone(), frame)
      .await?;
    Ok(())
  }
}

#[derive(Debug)]
pub enum ClientInfoAck {
  Pending,
  Received(proto::flo_connect::PacketGameStartPlayerClientInfoRequest),
}

pub struct StartGameState {
  game_id: i32,
  player_ack_map: Option<HashMap<i32, ClientInfoAck>>,
  game_addr: Addr<GameActor>,
}

#[async_trait]
impl Actor for StartGameState {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    ctx.spawn({
      let addr = ctx.addr();
      async move {
        delay_for(TIMEOUT).await;
        addr.send(AckTimeout).await.ok();
      }
    });
  }
}

impl StartGameState {
  pub fn new(game_id: i32, game_addr: Addr<GameActor>, player_ids: Vec<i32>) -> Self {
    StartGameState {
      game_id,
      player_ack_map: Some(
        player_ids
          .into_iter()
          .map(|player_id| (player_id, ClientInfoAck::Pending))
          .collect(),
      ),
      game_addr,
    }
  }

  async fn ack_player_and_check(
    &mut self,
    player_id: i32,
    pkt: proto::flo_connect::PacketGameStartPlayerClientInfoRequest,
  ) -> Result<Option<StartGameCheckProceed>> {
    let done_map = {
      let done = if let Some(map) = self.player_ack_map.as_mut() {
        map.insert(player_id, ClientInfoAck::Received(pkt));

        if map.values().all(|ack| {
          if let ClientInfoAck::Received(_) = ack {
            true
          } else {
            false
          }
        }) {
          true
        } else {
          false
        }
      } else {
        // already done
        false
      };

      if done {
        if let Some(map) = self.player_ack_map.take() {
          map
            .into_iter()
            .filter_map(|(player_id, ack)| {
              if let ClientInfoAck::Received(pkt) = ack {
                Some((player_id, pkt))
              } else {
                None
              }
            })
            .collect::<HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>>()
            .into()
        } else {
          None
        }
      } else {
        None
      }
    };

    if let Some(map) = done_map {
      Ok(Some(StartGameCheckProceed { map }))
    } else {
      Ok(None)
    }
  }

  pub fn get_map(
    &self,
  ) -> Option<HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>> {
    self.player_ack_map.as_ref().map(|map| {
      map
        .iter()
        .filter_map(|(player_id, ack)| {
          if let ClientInfoAck::Received(ref pkt) = ack {
            Some((*player_id, pkt.clone()))
          } else {
            None
          }
        })
        .collect()
    })
  }
}

struct StartGamePlayerAckInner {
  pub player_id: i32,
  pub packet: proto::flo_connect::PacketGameStartPlayerClientInfoRequest,
}

impl Message for StartGamePlayerAckInner {
  type Result = Result<Option<StartGameCheckProceed>>;
}

#[async_trait]
impl Handler<StartGamePlayerAckInner> for StartGameState {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    StartGamePlayerAckInner { player_id, packet }: StartGamePlayerAckInner,
  ) -> <StartGamePlayerAckInner as Message>::Result {
    tracing::debug!(player_id, "ack");
    self.ack_player_and_check(player_id, packet).await
  }
}

pub struct StartGamePlayerAck(StartGamePlayerAckInner);
impl StartGamePlayerAck {
  pub fn new(
    player_id: i32,
    packet: proto::flo_connect::PacketGameStartPlayerClientInfoRequest,
  ) -> Self {
    Self(StartGamePlayerAckInner { player_id, packet })
  }
}

impl Message for StartGamePlayerAck {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartGamePlayerAck> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    StartGamePlayerAck(message): StartGamePlayerAck,
  ) -> <StartGamePlayerAck as Message>::Result {
    let res = self
      .start_state
      .as_ref()
      .ok_or_else(|| Error::GameNotStarting)?
      .send(message)
      .await??;

    if let Some(proceed) = res {
      self.start_state.take();
      if let Err(err) = self.start_game_proceed(proceed).await {
        let pkt = proto::flo_connect::PacketGameStartReject {
          game_id: self.game_id,
          message: format!("Internal error: {}", err),
          ..Default::default()
        };
        self
          .player_packet_sender
          .send(self.host_player, pkt.encode_as_frame()?)
          .await?;
      }
    }

    Ok(())
  }
}

struct AckTimeout;
impl Message for AckTimeout {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<AckTimeout> for StartGameState {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: AckTimeout,
  ) -> <AckTimeout as Message>::Result {
    if let Some(map) = self.get_map() {
      tracing::debug!(game_id = self.game_id, "ack timeout");
      self.game_addr.send(StartGameCheckTimeout { map }).await??;
    }
    Ok(())
  }
}
