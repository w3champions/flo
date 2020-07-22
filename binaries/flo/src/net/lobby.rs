use futures::stream::StreamExt;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tracing_futures::Instrument;

pub use flo_net::connect::*;
use flo_net::packet::{Frame, PacketTypeId};
use flo_net::stream::FloStream;

use crate::error::{Error, Result};
use crate::ws::{message, OutgoingMessage, WsSenderRef};

#[derive(Debug)]
pub struct LobbyStream {
  session: Arc<PlayerSession>,
  frame_sender: mpsc::Sender<Frame>,
  ws_sender: WsSenderRef,
}

impl LobbyStream {
  pub async fn connect(domain: &str, ws_sender: WsSenderRef, token: String) -> Result<Self> {
    let addr = format!("{}:{}", domain, flo_constants::LOBBY_SOCKET_PORT);

    tracing::debug!("connect addr: {}", addr);

    let mut stream = FloStream::connect(addr).await?;

    stream
      .send(PacketConnectLobby {
        connect_version: Some(crate::version::FLO_VERSION.into()),
        token,
      })
      .await?;

    let reply = stream.recv_frame().await?;

    let session = flo_net::frame_packet! {
      reply => {
        p = PacketConnectLobbyAccept => {
          PlayerSession::unpack(p.session)?
        },
        p = PacketConnectLobbyReject => {
          return Err(Error::ConnectionRequestRejected(RejectReason::unpack(p.reason)?))
        },
      }
    };

    let session = Arc::new(session);
    let (frame_sender, mut frame_r) = mpsc::channel(5);

    if let SendResult::Closed =
      Self::send_event(&ws_sender, LobbyEvent::Connected(session.clone())).await?
    {
      return Err(Error::WebsocketClosed);
    }

    tokio::spawn({
      let ws_sender = ws_sender.clone();
      async move {
        loop {
          tokio::select! {
            next_send = frame_r.next() => {
              if let Some(frame) = next_send {
                match stream.send_frame(frame).await {
                  Ok(_) => {},
                  Err(e) => {
                    tracing::debug!("exiting: send error: {}", e);
                    break;
                  }
                }
              } else {
                tracing::debug!("exiting: sender dropped");
                break;
              }
            }
            recv = stream.recv_frame() => {
              match recv {
                Ok(frame) => {
                  match Self::dispatch(&ws_sender, frame).await {
                    Ok(_) => {},
                    Err(e) => {
                      tracing::debug!("exiting: dispatch: {}", e);
                      break;
                    }
                  }
                },
                Err(e) => {
                  tracing::debug!("exiting: recv: {}", e);
                  match Self::send_event(&ws_sender, LobbyEvent::Disconnect(
                    DisconnectReason::Unknown
                  )).await {
                    Ok(_) => {},
                    Err(e) => {
                      tracing::debug!("exiting: send disconnect: {}", e);
                    }
                  }
                  break;
                }
              }
            }
          }
        }
        tracing::debug!("dropped")
      }
      .instrument(tracing::debug_span!("worker"))
    });

    Ok(LobbyStream {
      session,
      frame_sender,
      ws_sender,
    })
  }

  async fn dispatch(sender: &WsSenderRef, frame: Frame) -> Result<SendResult> {
    let event = flo_net::frame_packet! {
      frame => {
        p = PacketLobbyDisconnect => {
          LobbyEvent::Disconnect(S2ProtoUnpack::unpack(p.reason)?)
        },
      }
    };

    Self::send_event(sender, event).await
  }

  async fn send_event(sender: &WsSenderRef, evt: LobbyEvent) -> Result<SendResult> {
    let msg = match evt {
      LobbyEvent::Connected(session) => OutgoingMessage::PlayerSession(session.clone()),
      LobbyEvent::Disconnect(reason) => OutgoingMessage::Disconnect(message::Disconnect { reason }),
      LobbyEvent::Invitation => OutgoingMessage::Invitation,
    };
    if let Err(err) = sender.send(msg).await {
      tracing::error!("send event: {}", err);
      Ok(SendResult::Closed)
    } else {
      Ok(SendResult::Ok)
    }
  }
}

#[derive(Debug)]
enum SendResult {
  Ok,
  Closed,
}

#[derive(Debug)]
pub enum LobbyEvent {
  Connected(Arc<PlayerSession>),
  Invitation,
  Disconnect(DisconnectReason),
}

#[derive(Debug)]
pub enum LobbyRequest {}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::LobbyDisconnectReason")]
pub enum DisconnectReason {
  Unknown = 0,
  Multi = 1,
  Maintenance = 2,
}

#[derive(Debug)]
pub struct Invitation;

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::Session")]
pub struct PlayerSession {
  pub player: PlayerInfo,
  pub status: PlayerStatus,
  pub game_id: Option<i32>,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerStatus")]
pub enum PlayerStatus {
  Idle = 0,
  InGame = 1,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::PlayerInfo")]
pub struct PlayerInfo {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerSource")]
pub enum PlayerSource {
  Test = 0,
  BNet = 1,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::ConnectLobbyRejectReason")]
pub enum RejectReason {
  Unknown = 0,
  ClientVersionTooOld = 1,
  InvalidToken = 2,
}
