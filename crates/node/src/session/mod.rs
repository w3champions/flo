mod types;
pub use types::*;

use dashmap::DashMap;
use lazy_static::lazy_static;
use parking_lot::RwLockWriteGuard;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use flo_net::packet::{FloPacket, Frame, OptionalFieldExt};
use flo_net::proto::flo_node::{
  ClientConnectRejectReason, ControllerCreateGameRejectReason, Game, PacketClientConnect,
  PacketClientConnectAccept, PacketClientConnectReject, PacketControllerConnectReject,
  PacketControllerCreateGame, PacketControllerCreateGameAccept, PacketControllerCreateGameReject,
};

use crate::error::*;
use crate::metrics;

#[derive(Debug)]
pub struct SessionStore {
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
    SessionStore {
      pending_players: PendingPlayerRegistry::new(),
      connected_players: DashMap::new(),
      games: GameRegistry::new(),
    }
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

    if let Err(err) = self.games.register(game) {
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

    metrics::GAME_SESSIONS.inc();
    metrics::PENDING_PLAYER_TOKENS.add(player_tokens.len() as i64);

    Ok(
      PacketControllerCreateGameAccept {
        game_id,
        player_tokens,
      }
      .encode_as_frame()?,
    )
  }

  pub fn get_pending_player(&self, token: &PlayerToken) -> Option<PendingPlayer> {
    self.pending_players.get_by_token(token)
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
  map: DashMap<i32, Arc<RwLock<GameSession>>>,
}

impl GameRegistry {
  fn new() -> Self {
    GameRegistry {
      map: DashMap::new(),
    }
  }

  // for controller
  fn register(&self, game: Game) -> Result<()> {
    let game_id = game.id;

    if let Some(game) = self.map.get(&game_id) {
      if game.read().status != GameStatus::Created {
        return Err(Error::GameExists);
      }
    }

    let session = Arc::new(RwLock::new(GameSession::new(game)?));

    if self.map.insert(game_id, session).is_none() {
      metrics::GAME_SESSIONS.inc();
    }

    Ok(())
  }
}
