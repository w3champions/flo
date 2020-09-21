use crate::error::*;
use crate::game::{Game, SlotClientStatus, SlotStatus};
use crate::node::PlayerToken;
use flo_net::packet::*;
use flo_net::proto::flo_node::*;

use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};

use futures::FutureExt;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::collections::HashMap;
use std::future::Future;

use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::{mpsc, oneshot};
use tokio::time::delay_for;
use tracing_futures::Instrument;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct NodeRequestActor {
  frame_tx: mpsc::Sender<Frame>,
  pending_requests: HashMap<RequestId, PendingRequest>,
}

impl Actor for NodeRequestActor {}

impl NodeRequestActor {
  pub fn new(frame_tx: mpsc::Sender<Frame>) -> Self {
    Self {
      frame_tx,
      pending_requests: HashMap::new(),
    }
  }
}

pub(crate) struct PendingRequest {
  tx: Option<oneshot::Sender<Result<Response>>>,
  done: Arc<Notify>,
}

impl Drop for PendingRequest {
  fn drop(&mut self) {
    if let Some(sender) = self.tx.take() {
      sender.send(Err(Error::NodeRequestCancelled)).ok();
    }
  }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum RequestId {
  CreateGame(i32),
  PlayerLeave(PlayerLeaveRequestId),
}

#[derive(Debug)]
pub enum Response {
  GameCreated(CreatedGameInfo),
  PlayerLeave(PlayerLeaveResponse),
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::PacketControllerCreateGameAccept))]
pub struct CreatedGameInfo {
  pub game_id: i32,
  pub player_tokens: Vec<PlayerToken>,
}

impl S2ProtoUnpack<flo_net::proto::flo_node::PlayerToken> for PlayerToken {
  fn unpack(
    value: flo_net::proto::flo_node::PlayerToken,
  ) -> Result<Self, s2_grpc_utils::result::Error> {
    let mut bytes = [0_u8; 16];
    if value.token.len() >= 16 {
      bytes.clone_from_slice(&value.token[0..16]);
    } else {
      (&mut bytes[0..(value.token.len())]).clone_from_slice(&value.token[0..(value.token.len())]);
    }
    Ok(PlayerToken {
      player_id: value.player_id,
      bytes,
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerLeaveRequestId {
  pub game_id: i32,
  pub player_id: i32,
}

#[derive(Debug)]
pub enum PlayerLeaveResponse {
  Accepted(SlotClientStatus),
  Rejected(UpdateSlotClientStatusRejectReason),
}

struct Request {
  id: RequestId,
  frame: Frame,
}

impl Message for Request {
  type Result = Result<PendingResponse>;
}

#[async_trait]
impl Handler<Request> for NodeRequestActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    Request { id, frame }: Request,
  ) -> Result<PendingResponse> {
    if self.pending_requests.contains_key(&id) {
      return Err(Error::NodeRequestProcessing);
    }

    let (tx, rx) = oneshot::channel();
    let done = Arc::new(Notify::new());
    let pending = PendingRequest {
      tx: Some(tx),
      done: done.clone(),
    };
    self.pending_requests.insert(id, pending);

    ctx
      .spawn({
        let addr = ctx.addr();
        let mut frame_tx = self.frame_tx.clone();
        async move {
          let timeout = delay_for(REQUEST_TIMEOUT);
          let send = frame_tx.send(frame);
          tracing::debug!("request sent: {:?}", id);

          tokio::pin!(timeout);
          tokio::pin!(send);

          // send frame
          let exit = tokio::select! {
            _ = &mut timeout => {
              request_callback(&addr, id, Err(Error::NodeRequestTimeout)).await;
              tracing::debug!("action timeout: {:?}", id);
              true
            }
            res = &mut send => {
              match res {
                Ok(_) => {
                  false
                }
                Err(_) => {
                  request_callback(&addr, id, Err(Error::NodeRequestCancelled)).await;
                  tracing::debug!("action error: {:?}", id);
                  true
                }
              }
            }
          };

          if !exit {
            tokio::select! {
              _ = done.notified() => {}
              _ = &mut timeout => {
                request_callback(&addr, id, Err(Error::NodeRequestTimeout)).await;
                tracing::debug!("response timeout: {:?}", id);
              }
            }
          }
        }
      })
      .instrument(tracing::debug_span!("worker"));
    Ok(PendingResponse(rx))
  }
}

async fn request_callback(addr: &Addr<NodeRequestActor>, id: RequestId, result: Result<Response>) {
  if addr.send(RequestDone { id, result }).await.is_err() {
    tracing::debug!("RequestDone: cancelled: request_id = {:?}", id);
  }
}

pub struct RequestDone {
  pub(crate) id: RequestId,
  result: Result<Response>,
}

impl RequestDone {
  pub fn new(id: RequestId, result: Result<Response>) -> Self {
    RequestDone { id, result }
  }
}

impl Message for RequestDone {
  type Result = ();
}

#[async_trait]
impl Handler<RequestDone> for NodeRequestActor {
  async fn handle(&mut self, _ctx: &mut Context<Self>, RequestDone { id, result }: RequestDone) {
    let pending = self.pending_requests.remove(&id);
    if let Some(mut pending) = pending {
      tracing::debug!("response: {:?}", id);
      if let Some(tx) = pending.tx.take() {
        pending.done.notify();
        tx.send(result).ok();
      }
    } else {
      tracing::debug!("response discarded: {:?}", id);
    }
  }
}

pub struct PendingResponse(oneshot::Receiver<Result<Response>>);

impl Future for PendingResponse {
  type Output = Result<Response>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
    self.0.poll_unpin(cx).map(|res| match res {
      Ok(res) => res,
      Err(_) => Err(Error::TaskCancelled),
    })
  }
}

#[async_trait]
pub trait NodeRequestExt {
  async fn create_game(&self, game: Game) -> Result<CreatedGameInfo>;
  async fn player_force_leave(&self, game_id: i32, player_id: i32) -> Result<PlayerLeaveResponse>;
}

#[async_trait]
impl NodeRequestExt for Addr<NodeRequestActor> {
  async fn create_game(&self, game: Game) -> Result<CreatedGameInfo> {
    let game_id = game.id;

    let req_id = RequestId::CreateGame(game_id);

    let mut slots = Vec::with_capacity(game.slots.len());
    for (i, slot) in game.slots.iter().enumerate() {
      if slot.settings.status == SlotStatus::Occupied {
        slots.push(flo_net::proto::flo_node::GameSlot {
          id: i as u32,
          player: slot.player.as_ref().map(|player| GamePlayer {
            player_id: player.id,
            name: player.name.clone(),
          }),
          settings: Some(slot.settings.clone().pack()?),
          client_status: Default::default(),
        });
      }
    }

    let pkt = PacketControllerCreateGame {
      game: Some(flo_net::proto::flo_node::Game {
        id: game_id,
        settings: Some(flo_net::proto::flo_node::GameSettings {
          map_path: game.map.path.clone(),
          map_sha1: game.map.sha1.to_vec(),
          map_checksum: game.map.checksum,
        }),
        slots,
        status: Default::default(),
      }),
    };

    let req = Request {
      id: req_id,
      frame: pkt.encode_as_frame()?,
    };

    let res = self.send(req).await??;
    match res.await? {
      Response::GameCreated(game_info) => Ok(game_info),
      other => {
        tracing::error!(game_id, "unexpected node response: {:?}", other);
        Err(Error::NodeResponseUnexpected)
      }
    }
  }

  async fn player_force_leave(&self, game_id: i32, player_id: i32) -> Result<PlayerLeaveResponse> {
    let req_id = RequestId::PlayerLeave(PlayerLeaveRequestId { game_id, player_id });

    let mut pkt = PacketControllerUpdateSlotStatus {
      player_id,
      game_id,
      ..Default::default()
    };

    pkt.set_status(flo_net::proto::flo_common::SlotClientStatus::Left);

    let req = Request {
      id: req_id,
      frame: pkt.encode_as_frame()?,
    };

    let res = self.send(req).await??;
    match res.await? {
      Response::PlayerLeave(res) => Ok(res),
      other => {
        tracing::error!(game_id, "unexpected node response: {:?}", other);
        Err(Error::NodeResponseUnexpected)
      }
    }
  }
}
