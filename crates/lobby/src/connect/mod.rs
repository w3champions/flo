use futures::future::abortable;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::stream::StreamExt;
use tokio::sync::Notify;
use tokio::time::delay_for;

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
        tracing::debug!("stream error: {}", err);
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

        flo_net::try_flo_packet! {
          frame => {
            packet = proto::flo_common::PacketPong => {
              // tracing::debug!("pong, latency = {}", stop_watch.elapsed_ms().saturating_sub(packet.ms));
            }
            packet = proto::flo_connect::PacketGameSlotUpdateRequest => {
              handle_game_slot_update_request(state.clone(), player_id, packet).await?;
            }
            _packet = proto::flo_connect::PacketListNodesRequest => {
              handle_list_nodes_request(state.clone(), player_id).await?;
            }
            packet = proto::flo_connect::PacketGamePlayerPingMapUpdateRequest => {
              handle_game_player_ping_map_update_request(state.clone(), player_id, packet).await?;
            }
            packet = proto::flo_connect::PacketGamePlayerPingMapSnapshotRequest => {
              handle_game_player_ping_map_snapshot_request(state.clone(), player_id, packet.game_id).await?;
            }
            packet = proto::flo_connect::PacketGameSelectNodeRequest => {
              handle_game_select_node_request(state.clone(), player_id, packet).await?;
            }
            packet = flo_net::proto::flo_connect::PacketGameStartRequest => {
              handle_game_start_request(state.clone(), player_id, packet).await?;
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

  let mut frames = vec![connect::PacketConnectLobbyAccept {
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
    nodes: state.config.with_nodes(|nodes| nodes.to_vec()).pack()?,
  }
  .encode_as_frame()?];

  if let Some(game_id) = game_id {
    let (game, node_player_token) = state
      .db
      .exec(move |conn| crate::game::db::get_full_and_node_player_token(conn, game_id, player_id))
      .await?;

    let game = game.into_packet();
    let frame = connect::PacketGameInfo { game: Some(game) }.encode_as_frame()?;
    frames.push(frame);

    if let Some(player_token) = node_player_token {
      let frame = connect::PacketGamePlayerToken {
        game_id,
        player_token: player_token.to_vec(),
      }
      .encode_as_frame()?;
      frames.push(frame);
    }
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

async fn handle_game_player_ping_map_update_request(
  state: LobbyStateRef,
  player_id: i32,
  packet: proto::flo_connect::PacketGamePlayerPingMapUpdateRequest,
) -> Result<()> {
  let game_id = packet.game_id;
  state
    .mem
    .update_game_player_ping_map(game_id, player_id, packet.ping_map.clone());

  // broadcast ping update when
  // - game node selected
  // - packet contains data related to the selected node
  let select_node_id = state.mem.get_game_selected_node(game_id);
  if let Some(select_node_id) = select_node_id {
    if packet.ping_map.contains_key(&select_node_id) {
      let mut player_ids = state.mem.get_game_player_ids(game_id);

      if player_ids.is_empty() {
        return Ok(());
      }

      player_ids.retain(|id| *id != player_id);
      if player_ids.len() > 0 {
        let mut senders = state.mem.get_player_senders(&player_ids);
        for sender in senders.values_mut() {
          sender.with_buf(|buf| {
            buf.add_ping_update(game_id, player_id, packet.ping_map.clone());
          });
        }
      }
    }
  }
  Ok(())
}

async fn handle_game_player_ping_map_snapshot_request(
  state: LobbyStateRef,
  player_id: i32,
  game_id: i32,
) -> Result<()> {
  use crate::state::GamePlayerPingMapKey;
  use flo_net::proto::flo_connect::*;

  let mut sender = if let Some(sender) = state.mem.get_player_sender(player_id) {
    sender
  } else {
    return Ok(());
  };
  if let Some(map) = state.mem.get_game_player_ping_map(game_id) {
    let mut node_ping_map = HashMap::<i32, NodePingMap>::new();
    for (GamePlayerPingMapKey { node_id, player_id }, ping) in map {
      let item = node_ping_map
        .entry(node_id)
        .or_insert_with(|| Default::default());
      item.player_ping_map.insert(player_id, ping);
    }
    sender
      .send(PacketGamePlayerPingMapSnapshot {
        game_id,
        node_ping_map,
      })
      .await?;
  } else {
    sender
      .send(PacketGamePlayerPingMapSnapshot {
        game_id,
        node_ping_map: Default::default(),
      })
      .await?;
  }
  Ok(())
}

async fn handle_game_select_node_request(
  state: LobbyStateRef,
  player_id: i32,
  packet: proto::flo_connect::PacketGameSelectNodeRequest,
) -> Result<()> {
  crate::game::select_game_node(state, packet.game_id, player_id, packet.node_id).await?;
  Ok(())
}

async fn handle_game_start_request(
  state: LobbyStateRef,
  player_id: i32,
  packet: proto::flo_connect::PacketGameStartRequest,
) -> Result<()> {
  crate::game::start_game(state, packet.game_id, player_id).await?;
  Ok(())
}
