mod action;
mod dispatch;

use dispatch::{Dispatcher, Message};

use s2_grpc_utils::S2ProtoEnum;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

use flo_net::packet::*;
use flo_task::{SpawnScope, SpawnScopeHandle};

use crate::error::*;
use crate::game::peer::{PeerMessage, PeerStream};
use crate::game::{GameEventSender, GameSlot, NodeGameStatus, NodeGameStatusSnapshot};

#[derive(Debug)]
pub struct GameHost {
  scope: SpawnScope,
  state: Arc<State>,
  game_status: NodeGameStatus,
  command_sender: Sender<Command>,
  dispatcher: Dispatcher,
}

impl GameHost {
  pub fn new(game_id: i32, slots: &[GameSlot], event_sender: GameEventSender) -> Self {
    let scope = SpawnScope::new();
    let (command_sender, command_receiver) = channel(5);
    let (dispatch_tx, dispatch_rx) = channel(crate::constants::GAME_DISPATCH_BUF_SIZE);
    let state = Arc::new(State {
      game_id,
      event_sender: event_sender.clone(),
      dispatch_tx,
      player_slot_id_map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, (slot.id + 1) as u8))
        .collect(),
    });

    tokio::spawn(
      Self::serve_commands(state.clone(), command_receiver, scope.handle())
        .instrument(tracing::debug_span!("command_worker", game_id)),
    );

    let dispatcher = Dispatcher::new(game_id, slots, dispatch_rx, event_sender);
    Self {
      scope,
      state,
      game_status: NodeGameStatus::Created,
      command_sender,
      dispatcher,
    }
  }

  pub fn start_dispatch(&mut self) {
    self.dispatcher.start();
  }

  pub async fn add_peer_stream(
    &mut self,
    snapshot: NodeGameStatusSnapshot,
    stream: PeerStream,
  ) -> Result<(), PeerStream> {
    self
      .command_sender
      .send(Command::AddPeerStream(AddPeerStream { snapshot, stream }))
      .await
      .map_err(|err| {
        let Command::AddPeerStream(AddPeerStream { stream, .. }) = err.0;
        stream
      })?;
    Ok(())
  }

  async fn serve_commands(
    state: Arc<State>,
    mut command_receiver: Receiver<Command>,
    mut scope: SpawnScopeHandle,
  ) {
    loop {
      tokio::select! {
        _ = scope.left() => {
          break;
        }
        next = command_receiver.recv() => {
          match next {
            Some(cmd) => {
              if let Err(err) = Self::handle_command(&state, cmd).await {
                tracing::debug!("handle command: {}", err);
              }
            },
            None => {
              // sender gone, dropped
              break;
            }
          }
        }
      }
    }
    tracing::debug!("exiting")
  }

  async fn serve_peer(
    state: &State,
    mut stream: PeerStream,
    snapshot: NodeGameStatusSnapshot,
    dispatch_tx: &mut Sender<Message>,
  ) -> Result<()> {
    let player_id = stream.player_id();
    let slot_player_id = state
      .player_slot_id_map
      .get(&player_id)
      .cloned()
      .ok_or_else(|| Error::PlayerNotFoundInGame)?;

    stream
      .get_mut()
      .send_frames(vec![{
        let mut pkt = flo_net::proto::flo_node::PacketClientConnectAccept {
          version: Some(crate::version::FLO_NODE_VERSION.into()),
          game_id: state.game_id,
          player_id,
          ..Default::default()
        };
        pkt.set_game_status(snapshot.game_status.into_proto_enum());
        for (player_id, status) in snapshot.player_game_client_status_map {
          pkt.insert_player_game_client_status_map(player_id, status.into_proto_enum());
        }
        pkt
      }
      .encode_as_frame()?])
      .await?;

    let (tx, mut rx) = channel(crate::constants::PEER_W3GS_CHANNEL_SIZE);
    dispatch_tx
      .send(Message::PlayerConnect {
        player_id: stream.player_id(),
        tx,
      })
      .await
      .map_err(|_| Error::Cancelled)?;

    loop {
      tokio::select! {
        next = stream.try_next() => {
          match next? {
            Some(msg) => {
              match msg {
                PeerMessage::Incoming(frame) => {
                  if dispatch_tx.send(Message::Incoming { player_id, slot_player_id, frame}).await.is_err() {
                    break;
                  }
                },
                PeerMessage::Outgoing(frame) => {
                  stream.get_mut().send_frame(frame).await?;
                }
              }
            }
            None => break,
          }
        }
        next = rx.recv() => {
          match next {
            Some(frame) => {
              stream.get_mut().send_frame(frame).await?;
            }
            None => break,
          }
        }
      }
    }

    Ok(())
  }

  async fn handle_command(state: &Arc<State>, cmd: Command) -> Result<()> {
    let game_id = state.game_id;
    match cmd {
      Command::AddPeerStream(AddPeerStream { snapshot, stream }) => {
        let player_id = stream.player_id();
        tracing::debug!(player_id, "add peer stream");
        let state = state.clone();
        tokio::spawn(
          async move {
            let mut dispatch_tx = state.dispatch_tx.clone();

            if let Err(err) = Self::serve_peer(&state, stream, snapshot, &mut dispatch_tx).await {
              tracing::debug!("peer: {}", err);
            }
            crate::metrics::CONNECTED_PLAYERS.dec();

            dispatch_tx
              .send(Message::PlayerDisconnect { player_id })
              .await
              .ok();
            tracing::debug!("exiting");
          }
          .instrument(tracing::debug_span!("peer_worker", game_id, player_id)),
        );
      }
    }
    Ok(())
  }
}

#[derive(Debug)]
struct State {
  game_id: i32,
  event_sender: GameEventSender,
  dispatch_tx: Sender<Message>,
  player_slot_id_map: HashMap<i32, u8>,
}

#[derive(Debug)]
enum Command {
  AddPeerStream(AddPeerStream),
}

#[derive(Debug)]
struct AddPeerStream {
  snapshot: NodeGameStatusSnapshot,
  stream: PeerStream,
}
