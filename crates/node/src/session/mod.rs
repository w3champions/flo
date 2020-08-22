mod event;
pub use event::{handle_events, NodeEvent, NodeEventSender};
mod types;
pub use types::*;

use dashmap::DashMap;
use lazy_static::lazy_static;
use parking_lot::RwLockWriteGuard;
use parking_lot::{Mutex, RwLock};
use s2_grpc_utils::S2ProtoEnum;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tracing_futures::Instrument;

use flo_event::FloEvent;
use flo_net::packet::{FloPacket, Frame, OptionalFieldExt};
use flo_net::proto::flo_node::{
  ClientConnectRejectReason, ControllerCreateGameRejectReason, Game, PacketClientConnect,
  PacketClientConnectAccept, PacketClientConnectReject, PacketControllerConnectReject,
  PacketControllerCreateGame, PacketControllerCreateGameAccept, PacketControllerCreateGameReject,
  PacketControllerUpdateSlotStatus, PacketControllerUpdateSlotStatusAccept,
  PacketControllerUpdateSlotStatusReject,
};

use crate::error::*;
use crate::game::{GameEventSender, GameSessionRef, GameStatus};
use crate::metrics;

#[derive(Debug)]
pub struct SessionStore {
  event_sender: NodeEventSender,
  pending_players: PendingPlayerRegistry,
  connected_players: DashMap<i32, ConnectedPlayer>,
  games: GameRegistry,
}

impl SessionStore {
  pub fn get() -> &'static SessionStore {
    lazy_static! {
      static ref INSTANCE: SessionStore = SessionStore::new();
    }

    &INSTANCE
  }

  fn new() -> Self {
    let (event_sender, event_receiver) = NodeEvent::channel(100);

    tokio::spawn(
      handle_events(event_receiver).instrument(tracing::debug_span!("event_handler_worker")),
    );

    SessionStore {
      event_sender,
      pending_players: PendingPlayerRegistry::new(),
      connected_players: DashMap::new(),
      games: GameRegistry::new(),
    }
  }

  pub fn get_pending_player(&self, token: &PlayerToken) -> Option<PendingPlayer> {
    self.pending_players.get_by_token(token)
  }

  pub fn handle_controller_create_game(&self, packet: PacketControllerCreateGame) -> Result<Frame> {
    let game = packet.game.extract()?;
    let game_id = game.id;
    let player_ids: Vec<i32> = game
      .slots
      .iter()
      .filter_map(|s| s.player.as_ref().map(|p| p.player_id))
      .collect();

    if player_ids.is_empty() {
      return Err(Error::NoPlayer);
    }

    {
      for id in &player_ids {
        if self.connected_players.contains_key(id) {
          return Err(Error::PlayerBusy(*id));
        }
      }
    }

    let pending: Vec<(PlayerToken, PendingPlayer)> = {
      let players: Vec<_> = game
        .slots
        .iter()
        .filter_map(|s| s.player.as_ref())
        .collect();
      players
        .iter()
        .map(|p| {
          (
            PlayerToken::new_uuid(),
            PendingPlayer {
              player_id: p.player_id,
              game_id,
            },
          )
        })
        .collect()
    };

    if let Err(err) = self
      .games
      .register_game(self.event_sender.clone().into(), game)
    {
      let reason = match err {
        Error::GameExists => ControllerCreateGameRejectReason::GameExists,
        Error::PlayerBusy(_) => ControllerCreateGameRejectReason::PlayerBusy,
        err => return Err(err),
      };
      return Ok(
        PacketControllerCreateGameReject {
          game_id,
          reason: reason.into(),
        }
        .encode_as_frame()?,
      );
    }

    let player_tokens: Vec<_> = pending
      .iter()
      .map(|(token, player)| flo_net::proto::flo_node::PlayerToken {
        player_id: player.player_id,
        token: token.to_vec(),
      })
      .collect();

    let stale_pending_players = self.pending_players.register(pending);
    if !stale_pending_players.is_empty() {
      for player in stale_pending_players {
        tracing::warn!(
          "stale player: player_id = {}, game_id = {}",
          player.player_id,
          player.game_id
        );
      }
    }

    metrics::PENDING_PLAYER_TOKENS.add(player_tokens.len() as i64);

    Ok(
      PacketControllerCreateGameAccept {
        game_id,
        player_tokens,
      }
      .encode_as_frame()?,
    )
  }

  pub async fn handle_controller_update_slot_client_status(
    &self,
    packet: PacketControllerUpdateSlotStatus,
  ) -> Result<Frame> {
    use flo_net::proto::flo_common::SlotClientStatus;
    use flo_net::proto::flo_node::UpdateSlotClientStatusRejectReason;
    let game_id = packet.game_id;
    let player_id = packet.player_id;
    let client_status = packet.status();

    if client_status != SlotClientStatus::Left {
      tracing::error!("controller can only change client status to leave");
      return Ok(
        PacketControllerUpdateSlotStatusReject {
          player_id,
          game_id,
          reason: UpdateSlotClientStatusRejectReason::InvalidStatus.into(),
        }
        .encode_as_frame()?,
      );
    }

    let game = match self.games.get(game_id) {
      Some(game) => game,
      None => {
        return Ok(
          PacketControllerUpdateSlotStatusReject {
            player_id,
            game_id,
            reason: UpdateSlotClientStatusRejectReason::NotFound.into(),
          }
          .encode_as_frame()?,
        );
      }
    };

    match game
      .update_player_client_status(player_id, S2ProtoEnum::unpack_enum(client_status))
      .await
    {
      Ok(_) => Ok(
        PacketControllerUpdateSlotStatusAccept {
          player_id,
          game_id,
          status: client_status.into(),
        }
        .encode_as_frame()?,
      ),
      Err(err) => match err {
        Error::InvalidClientStatusTransition => Ok(
          PacketControllerUpdateSlotStatusReject {
            player_id,
            game_id,
            reason: UpdateSlotClientStatusRejectReason::InvalidStatus.into(),
          }
          .encode_as_frame()?,
        ),
        err => {
          tracing::error!(game_id, player_id, "update client status: {}", err);
          Ok(
            PacketControllerUpdateSlotStatusReject {
              player_id,
              game_id,
              reason: UpdateSlotClientStatusRejectReason::Unknown.into(),
            }
            .encode_as_frame()?,
          )
        }
      },
    }
  }
}

#[derive(Debug)]
struct PendingPlayerRegistry {
  map: DashMap<PlayerToken, PendingPlayer>,
  player_token: DashMap<i32, PlayerToken>,
}

impl PendingPlayerRegistry {
  fn new() -> Self {
    PendingPlayerRegistry {
      map: DashMap::new(),
      player_token: DashMap::new(),
    }
  }

  // for controller
  fn register(&self, pairs: Vec<(PlayerToken, PendingPlayer)>) -> Vec<PendingPlayer> {
    use dashmap::mapref::entry::Entry;

    let mut stale_players = vec![];

    for (token, player) in pairs {
      let player_id = player.player_id;
      // remove stale player
      let stale_player = match self.player_token.entry(player_id) {
        // replace existing token
        Entry::Occupied(mut e) => {
          let r = e.get_mut();
          let prev_token = std::mem::replace(r, token.clone());
          self.map.remove(&prev_token)
        }
        // add token
        Entry::Vacant(e) => {
          e.insert(token.clone());
          metrics::PENDING_PLAYER_TOKENS.inc();
          None
        }
      };

      self.map.insert(token.clone(), player);

      if let Some((_, stale_player)) = stale_player {
        stale_players.push(stale_player)
      }
    }

    stale_players
  }

  pub fn get_by_token(&self, token: &PlayerToken) -> Option<PendingPlayer> {
    self.map.get(&token).as_ref().map(|r| r.value().clone())
  }
}

#[derive(Debug)]
struct GameRegistry {
  map: DashMap<i32, GameSessionRef>,
}

impl GameRegistry {
  fn new() -> Self {
    GameRegistry {
      map: DashMap::new(),
    }
  }

  // for controller
  fn register_game(&self, event_sender: GameEventSender, game: Game) -> Result<()> {
    use dashmap::mapref::entry::Entry;
    let game_id = game.id;

    match self.map.entry(game_id) {
      Entry::Vacant(entry) => {
        entry.insert(GameSessionRef::new(event_sender, game)?);
        metrics::GAME_SESSIONS.inc();
      }
      Entry::Occupied(_) => {}
    }
    Ok(())
  }

  fn get(&self, game_id: i32) -> Option<GameSessionRef> {
    self.map.get(&game_id).map(|r| r.value().clone())
  }
}
