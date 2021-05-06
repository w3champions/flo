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
use tokio::sync::oneshot;

use tokio::time::sleep;

const TIMEOUT: Duration = Duration::from_secs(10);

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

    self.start_state = StartGameState::new(game_id, ctx.addr(), players, None)
      .start()
      .into();

    let frame = proto::flo_connect::PacketGameStarting { game_id }.encode_as_frame()?;
    self
      .player_reg
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
  ) -> Result<Result<(), proto::flo_connect::PacketGameStartReject>> {
    let game_id = self.game_id;

    tracing::debug!(game_id, "start game check proceed.");

    let mut pass = true;
    let agreed_version: Option<String>;
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
      agreed_version = version.map(ToString::to_string);
    }

    if !pass {
      let pkt = proto::flo_connect::PacketGameStartReject {
        game_id,
        message: "Unable to start the game because the game and map version check failed."
          .to_string(),
        player_client_info_map: map.clone(),
      };
      let frame = pkt.encode_as_frame()?;
      self
        .player_reg
        .broadcast(self.players.clone(), frame)
        .await?;

      tracing::error!(
        game_id = self.game_id,
        "start game failed: version check failed"
      );

      return Ok(Err(pkt));
    }

    let (game, ban_list_map) = self
      .db
      .exec(move |conn| {
        let game = crate::game::db::get_full(conn, game_id)?;
        let players = game.get_player_ids();
        Ok::<_, Error>((game, crate::player::db::get_ban_list_map(conn, &players)?))
      })
      .await?;

    let node_id = if let Some(id) = game.node.as_ref().map(|node| node.id) {
      id
    } else {
      return Err(Error::GameNodeNotSelected);
    };

    let created = self
      .nodes
      .send_to(node_id, NodeCreateGame { game, ban_list_map })
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

        tracing::error!(game_id = self.game_id, "start game failed: {}", pkt.message);

        return Ok(Err(pkt));
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
    self.player_reg.broadcast_map(packet_iter).await?;

    self
      .db
      .exec(move |conn| crate::game::db::update_created(conn, game_id, agreed_version, token_map))
      .await?;
    self.status = GameStatus::Created;

    Ok(Ok(()))
  }
}

pub struct StartGameCheckTimeout {
  pub map: HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>,
}

impl Message for StartGameCheckTimeout {
  type Result = ();
}

#[async_trait]
impl Handler<StartGameCheckTimeout> for GameActor {
  async fn handle(&mut self, _: &mut Context<Self>, msg: StartGameCheckTimeout) {
    if let Err(err) = self.send_game_start_reject_timeout(msg).await {
      tracing::error!(
        game_id = self.game_id,
        "send_game_start_reject_timeout: {}",
        err
      );
    }
  }
}

impl GameActor {
  async fn send_game_start_reject_timeout(
    &mut self,
    StartGameCheckTimeout { map }: StartGameCheckTimeout,
  ) -> Result<()> {
    let game_id = self.game_id;
    let start_state = if let Some(v) = self.start_state.take() {
      v
    } else {
      tracing::debug!(game_id, "start state gone");
      return Ok(());
    };
    let start_state = start_state.shutdown().await?;

    let pkt = proto::flo_connect::PacketGameStartReject {
      game_id,
      message: "Some of the players didn't response in time.".to_string(),
      player_client_info_map: map,
    };
    let frame = pkt.encode_as_frame()?;

    if start_state.by_api() {
      start_state.reply_api(StartGameCheckAsBotResult::Rejected(pkt));
    }

    self
      .player_reg
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
  done: bool,
  game_id: i32,
  player_ack_map: Option<HashMap<i32, ClientInfoAck>>,
  game_addr: Addr<GameActor>,
  api_tx: Option<oneshot::Sender<StartGameCheckAsBotResult>>,
}

#[async_trait]
impl Actor for StartGameState {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    ctx.spawn({
      let addr = ctx.addr();
      async move {
        sleep(TIMEOUT).await;
        addr.send(AckTimeout).await.ok();
      }
    });
  }
}

impl StartGameState {
  pub fn new(
    game_id: i32,
    game_addr: Addr<GameActor>,
    player_ids: Vec<i32>,
    api_tx: Option<oneshot::Sender<StartGameCheckAsBotResult>>,
  ) -> Self {
    StartGameState {
      done: false,
      game_id,
      player_ack_map: Some(
        player_ids
          .into_iter()
          .map(|player_id| (player_id, ClientInfoAck::Pending))
          .collect(),
      ),
      game_addr,
      api_tx,
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
      self.done = true;
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

  fn by_api(&self) -> bool {
    self.api_tx.is_some()
  }

  fn reply_api(self, reply: StartGameCheckAsBotResult) {
    self.api_tx.map(move |tx| tx.send(reply).ok());
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
    if self.done {
      tracing::info!(
        game_id = self.game_id,
        player_id,
        "game player ack discarded: timeout"
      );
      return Ok(None);
    }

    tracing::info!(
      game_id = self.game_id,
      player_id,
      "start game ack: version = {}, map sha1 = {:02X?}",
      packet.war3_version,
      packet.map_sha1
    );
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
      let start_state = self
        .start_state
        .take()
        .ok_or_else(|| Error::GameNotStarting)?;
      let start_state = start_state.shutdown().await?;

      match self.start_game_proceed(proceed).await {
        Ok(Ok(_)) => {
          if start_state.by_api() {
            let map = start_state.get_map();
            start_state.reply_api(StartGameCheckAsBotResult::Started(map));
          }
        }
        Ok(Err(pkt)) => {
          if start_state.by_api() {
            start_state.reply_api(StartGameCheckAsBotResult::Rejected(pkt));
          } else {
            self
              .player_reg
              .send(self.host_player, pkt.encode_as_frame()?)
              .await?;
          }
        }
        Err(err) => {
          let pkt = proto::flo_connect::PacketGameStartReject {
            game_id: self.game_id,
            message: format!("Internal error: {}", err),
            ..Default::default()
          };
          self
            .player_reg
            .send(self.host_player, pkt.encode_as_frame()?)
            .await?;
          if start_state.by_api() {
            start_state.reply_api(StartGameCheckAsBotResult::Rejected(pkt));
          }
        }
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
    if self.done {
      return Ok(());
    }
    if let Some(map) = self.get_map() {
      tracing::debug!(game_id = self.game_id, "ack timeout");
      self.done = true;
      self.game_addr.notify(StartGameCheckTimeout { map }).await?;
    }
    Ok(())
  }
}

pub enum StartGameCheckAsBotResult {
  Started(Option<HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>>),
  Rejected(proto::flo_connect::PacketGameStartReject),
}

pub struct StartGameCheckAsBot {
  pub tx: oneshot::Sender<StartGameCheckAsBotResult>,
}

impl Message for StartGameCheckAsBot {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartGameCheckAsBot> for GameActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    StartGameCheckAsBot { tx }: StartGameCheckAsBot,
  ) -> <StartGameCheckAsBot as Message>::Result {
    let game_id = self.game_id;

    if self.selected_node_id.is_none() {
      return Err(Error::GameNodeNotSelected);
    }

    let players = self.players.clone();
    if self.start_state.is_some() {
      return Err(Error::GameStarted);
    }

    self.start_state = StartGameState::new(game_id, ctx.addr(), players, Some(tx))
      .start()
      .into();

    let frame = proto::flo_connect::PacketGameStarting { game_id }.encode_as_frame()?;
    self
      .player_reg
      .broadcast(self.players.clone(), frame)
      .await?;

    Ok(())
  }
}
