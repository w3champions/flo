pub use event::{handle_global_events, GlobalEvent, GlobalEventSender};
pub mod event;
mod types;

pub use types::*;

use dashmap::DashMap;
use parking_lot::RwLock;
use s2_grpc_utils::S2ProtoEnum;
use std::collections::HashMap;
use std::sync::Arc;

use flo_net::packet::{FloPacket, Frame, OptionalFieldExt};
use flo_net::proto::flo_node::{
  ControllerCreateGameRejectReason, Game, PacketControllerCreateGame,
  PacketControllerCreateGameAccept, PacketControllerCreateGameReject,
  PacketControllerUpdateSlotStatus, PacketControllerUpdateSlotStatusAccept,
  PacketControllerUpdateSlotStatusReject,
};

use crate::controller::ControllerServerHandle;
use crate::error::*;
use crate::game::{GameSession, GameSessionHandle, SlotClientStatusUpdateSource};
use crate::metrics;

#[derive(Debug)]
pub struct GlobalState {
  event_sender: GlobalEventSender,
  players: PlayerRegistry,
  games: GameRegistry,
}

pub type GlobalStateRef = Arc<GlobalState>;

impl GlobalState {
  pub fn new(event_sender: GlobalEventSender) -> Self {
    GlobalState {
      event_sender,
      players: PlayerRegistry::new(),
      games: GameRegistry::new(),
    }
  }

  pub fn into_ref(self) -> GlobalStateRef {
    Arc::new(self)
  }

  pub fn get_pending_player(&self, token: &PlayerToken) -> Option<RegisteredPlayer> {
    self.players.get_by_token(token)
  }

  pub fn get_game(&self, id: i32) -> Option<GameSessionHandle> {
    self.games.get(id)
  }

  pub fn end_game(&self, id: i32) {
    self.players.remove_game(id);
    self.games.remove(id);
  }

  pub fn handle_controller_create_game(
    &self,
    ctrl: ControllerServerHandle,
    packet: PacketControllerCreateGame,
  ) -> Result<Frame> {
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

    let pending: Vec<(PlayerToken, RegisteredPlayer)> = {
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
            RegisteredPlayer {
              player_id: p.player_id,
              game_id,
            },
          )
        })
        .collect()
    };

    if let Err(err) = self
      .games
      .register(game, ctrl, self.event_sender.clone().into())
    {
      let reason = match err {
        Error::GameExists => ControllerCreateGameRejectReason::GameExists,
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

    let stale_pending_players = self.players.register(GamePlayerTokens {
      game_id,
      pairs: pending,
    });
    if !stale_pending_players.is_empty() {
      for player in stale_pending_players {
        tracing::warn!(
          "stale player: player_id = {}, game_id = {}",
          player.player_id,
          player.game_id
        );
      }
    }

    metrics::PLAYER_TOKENS.add(player_tokens.len() as i64);

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
      tracing::error!(
        "controller can only change client status to Left, got {:?}",
        client_status
      );
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
      .update_player_client_status(
        SlotClientStatusUpdateSource::Controller,
        player_id,
        S2ProtoEnum::unpack_enum(client_status),
      )
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
        Error::InvalidClientStatusTransition(_, _) => Ok(
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
struct PlayerRegistry {
  state: RwLock<PlayerTokenRegistryState>,
}

#[derive(Debug, Default)]
struct PlayerTokenRegistryState {
  map: HashMap<PlayerToken, RegisteredPlayer>,
  player_token: HashMap<i32, PlayerToken>,
  // game_id => [(player_id, tokens)]
  game_tokens: HashMap<i32, Vec<(i32, PlayerToken)>>,
}

impl PlayerRegistry {
  fn new() -> Self {
    PlayerRegistry {
      state: RwLock::new(PlayerTokenRegistryState::default()),
    }
  }

  // for controller
  fn register(
    &self,
    GamePlayerTokens { game_id, pairs }: GamePlayerTokens,
  ) -> Vec<RegisteredPlayer> {
    let mut state = self.state.write();
    let mut stale_players = vec![];

    state.game_tokens.insert(game_id, {
      pairs
        .iter()
        .map(|(token, player)| (player.player_id, token.clone()))
        .collect()
    });

    for (token, player) in pairs {
      let player_id = player.player_id;
      let stale_player = if let Some(old) = state.player_token.insert(player_id, token.clone()) {
        state.map.remove(&old)
      } else {
        metrics::PLAYER_TOKENS.inc();
        None
      };
      state.map.insert(token.clone(), player);
      if let Some(stale_player) = stale_player {
        stale_players.push(stale_player)
      }
    }

    stale_players
  }

  fn remove_game(&self, game_id: i32) {
    let mut state = self.state.write();
    // remove game_id => tokens
    if let Some(tokens) = state.game_tokens.remove(&game_id) {
      for (player_id, token) in tokens {
        use std::collections::hash_map::Entry;
        // remove token => player
        if state.map.remove(&token).is_some() {
          metrics::PLAYER_TOKENS.dec();
        }
        // remote player_id => token
        match state.player_token.entry(player_id) {
          Entry::Occupied(entry) => {
            if entry.get() == &token {
              entry.remove();
            }
          }
          Entry::Vacant(_) => {}
        }
      }
    }
  }

  pub fn get_by_token(&self, token: &PlayerToken) -> Option<RegisteredPlayer> {
    self.state.read().map.get(&token).cloned()
  }
}

#[derive(Debug)]
struct GamePlayerTokens {
  game_id: i32,
  pairs: Vec<(PlayerToken, RegisteredPlayer)>,
}

#[derive(Debug)]
struct GameRegistry {
  map: DashMap<i32, GameSession>,
}

impl GameRegistry {
  fn new() -> Self {
    GameRegistry {
      map: DashMap::new(),
    }
  }

  // for controller
  fn register(
    &self,
    game: Game,
    ctrl: ControllerServerHandle,
    g_event_sender: GlobalEventSender,
  ) -> Result<()> {
    use dashmap::mapref::entry::Entry;
    let game_id = game.id;

    match self.map.entry(game_id) {
      Entry::Vacant(entry) => {
        entry.insert(GameSession::new(game, ctrl, g_event_sender)?);
        metrics::GAME_SESSIONS.inc();
      }
      Entry::Occupied(_) => {}
    }
    Ok(())
  }

  fn get(&self, game_id: i32) -> Option<GameSessionHandle> {
    self.map.get(&game_id).map(|r| r.value().handle())
  }

  fn remove(&self, id: i32) {
    if let Some(_) = self.map.remove(&id) {
      metrics::GAME_SESSIONS.dec();
    }
  }
}
