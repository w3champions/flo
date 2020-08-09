use futures::future::abortable;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

use flo_net::connect;
use flo_net::listener::FloListener;
use flo_net::packet::FloPacket;
use flo_net::packet::OptionalFieldExt;
use flo_net::proto;
use flo_net::stream::FloStream;
use flo_net::time::StopWatch;

use crate::error::*;
use crate::state::{LobbyStateRef, LockedPlayerState};

mod handshake;
mod send_buf;
mod state;
use crate::game::SlotSettings;
pub use state::{Message as PlayerSenderMessage, PlayerReceiver, PlayerSenderRef};
use tokio::stream::StreamExt;
use tokio::time::delay_for;

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PING_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn serve(state: LobbyStateRef) -> Result<()> {
  let mut listener = FloListener::bind_v4(crate::constants::LOBBY_SOCKET_PORT).await?;
  tracing::info!("listening on port {}", listener.port());

  while let Some(mut stream) = listener.incoming().try_next().await? {
    let state = state.clone();
    tokio::spawn(async move {
      tracing::debug!("connected: {}", stream.peer_addr()?);

      let accepted = match handshake::handle_handshake(&mut stream).await {
        Ok(accepted) => accepted,
        Err(e) => {
          tracing::debug!("dropping: handshake error: {}", e);
          return Ok(());
        }
      };

      let player_id = accepted.player_id;
      tracing::debug!("accepted: player_id = {}", player_id);

      let (sender, receiver) = {
        let (sender, r) = PlayerSenderRef::new(player_id);
        let mut player_state = state.mem.lock_player_state(player_id).await;
        player_state.replace_sender(sender.clone());
        (sender, r)
      };

      if let Err(err) = handle_stream(state.clone(), player_id, stream, receiver).await {
        tracing::warn!("stream error: {}", err);
      }

      state
        .mem
        .lock_player_state(player_id)
        .await
        .remove_sender(sender);

      tracing::debug!("exiting: player_id = {}", player_id);
      Ok::<_, crate::error::Error>(())
    });
  }

  tracing::info!("shutting down");

  Ok(())
}

#[tracing::instrument(target = "player_stream", skip(state, stream))]
async fn handle_stream(
  state: LobbyStateRef,
  player_id: i32,
  mut stream: FloStream,
  mut receiver: PlayerReceiver,
) -> Result<()> {
  send_initial_state(state.clone(), &mut stream, player_id).await?;

  let stop_watch = StopWatch::new();
  let ping_timeout_notify = Arc::new(Notify::new());
  let mut ping_timeout_abort = None;

  loop {
    let mut next_ping = delay_for(PING_INTERVAL);

    tokio::select! {
      _ = &mut next_ping => {
        let notify = ping_timeout_notify.clone();

        stream.send(proto::flo_common::PacketPing {
          ms: stop_watch.elapsed_ms()
        }).await?;
        let (set_ping_timeout, abort) = abortable(async move {
          delay_for(PING_TIMEOUT).await;
          notify.notify();
        });
        ping_timeout_abort = Some(abort);
        tokio::spawn(set_ping_timeout);
      }
      _ = ping_timeout_notify.notified() => {
          tracing::debug!("ping timeout");
          break;
      }
      outgoing = receiver.recv() => {
        if let Some(msg) = outgoing {
          if let Err(e) = match msg {
            PlayerSenderMessage::Frame(frame) => {
              stream.send_frame(frame).await
            }
            PlayerSenderMessage::Frames(frames) => {
              stream.send_frames(frames).await
            }
            PlayerSenderMessage::Broken => {
              tracing::debug!("sender broken");
              break;
            }
          } {
            tracing::debug!("send error: {}", e);
            break;
          }
        } else {
          tracing::debug!("sender dropped");
          break;
        }
      }
      incoming = stream.recv_frame() => {
        if let Some(abort) = ping_timeout_abort.take() {
          abort.abort();
        }

        let frame = incoming?;

        flo_net::frame_packet! {
          frame => {
            packet = proto::flo_common::PacketPong => {
              tracing::debug!("pong, latency = {}", stop_watch.elapsed_ms().saturating_sub(packet.ms));
            },
            packet = proto::flo_connect::PacketGameSlotUpdateRequest => {
              handle_game_slot_update_request(state.clone(), player_id, packet).await?;
            },
            _packet = proto::flo_connect::PacketListNodesRequest => {
              handle_list_nodes_request(state.clone(), player_id).await?;
            }
          }
        }
      }
    }
  }

  Ok(())
}

async fn send_initial_state(
  state: LobbyStateRef,
  stream: &mut FloStream,
  player_id: i32,
) -> Result<()> {
  let player = state
    .db
    .exec(move |conn| crate::player::db::get_ref(conn, player_id))
    .await?;

  let game_id = {
    let player = state.mem.lock_player_state(player.id).await;
    player.joined_game_id()
  };

  let mut frames = vec![
    connect::PacketConnectLobbyAccept {
      lobby_version: Some(From::from(crate::version::FLO_LOBBY_VERSION)),
      session: Some({
        use proto::flo_connect::*;
        Session {
          player: player.pack()?,
          status: if game_id.is_some() {
            PlayerStatus::InGame.into()
          } else {
            PlayerStatus::Idle.into()
          },
          game_id: game_id.clone(),
        }
      }),
    }
    .encode_as_frame()?,
    connect::PacketListNodes {
      nodes: state.config.with_nodes(|nodes| nodes.to_vec()).pack()?,
    }
    .encode_as_frame()?,
  ];

  if let Some(game_id) = game_id {
    let game = state
      .db
      .exec(move |conn| crate::game::db::get_full(conn, game_id))
      .await?
      .into_packet();
    let frame = connect::PacketGameInfo { game: Some(game) }.encode_as_frame()?;
    frames.push(frame);
  }

  stream.send_frames(frames).await?;
  Ok(())
}

impl LockedPlayerState {
  pub fn get_session_update(&self) -> proto::flo_connect::PacketPlayerSessionUpdate {
    use proto::flo_connect::*;
    let game_id = self.joined_game_id();
    PacketPlayerSessionUpdate {
      status: if game_id.is_some() {
        PlayerStatus::InGame.into()
      } else {
        PlayerStatus::Idle.into()
      },
      game_id,
    }
  }
}

async fn handle_game_slot_update_request(
  state: LobbyStateRef,
  player_id: i32,
  packet: proto::flo_connect::PacketGameSlotUpdateRequest,
) -> Result<()> {
  crate::game::update_game_slot_settings(
    state,
    packet.game_id,
    player_id,
    SlotSettings::unpack(packet.slot_settings.extract()?)?,
  )
  .await?;

  Ok(())
}

async fn handle_list_nodes_request(state: LobbyStateRef, player_id: i32) -> Result<()> {
  if let Some(mut sender) = state.mem.get_player_sender(player_id) {
    let nodes = state.config.with_nodes(|nodes| nodes.to_vec());
    let packet = proto::flo_connect::PacketListNodes {
      nodes: nodes.pack()?,
    };
    sender.with_buf(move |buf| buf.list_nodes(packet));
  }
  Ok(())
}
