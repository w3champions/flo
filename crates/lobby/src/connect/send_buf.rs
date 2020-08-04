use flo_net::packet::*;
use flo_net::proto::flo_connect::*;

use crate::error::Result;

const MAX_PACKET_LEN: usize = 32;

#[derive(Debug)]
pub struct PlayerSendBuf {
  player_id: i32,
  session_update: Option<PacketPlayerSessionUpdate>,
  game: GameData,
}

impl PlayerSendBuf {
  pub fn new(player_id: i32, current_game: Option<GameInfo>) -> Self {
    PlayerSendBuf {
      player_id,
      session_update: None,
      game: GameData::new(current_game),
    }
  }

  pub fn is_full(&self) -> bool {
    self.game.len() >= MAX_PACKET_LEN
  }

  pub fn update_session(&mut self, update: PacketPlayerSessionUpdate) {
    self.session_update = Some(update)
  }

  pub fn set_game(&mut self, next_game: GameInfo) {
    let span = tracing::warn_span!(
      "set_game",
      player_id = self.player_id,
      next_game_id = next_game.id
    );
    let _enter = span.enter();

    if let Some(current_game_id) = self.game.current_game_id.clone() {
      if next_game.id != current_game_id {
        tracing::warn!(
          current_game_id,
          "current game changed when player hasn't left"
        );
      }
    }

    self.game.current_game_id = Some(next_game.id);
    self.game.full_game = Some(PacketGameInfo {
      game: Some(next_game),
    });
    self.game.fragments.clear();
  }

  pub fn add_player_enter(
    &mut self,
    enter_game_id: i32,
    player: PlayerInfo,
    slot_index: i32,
    slot_settings: SlotSettings,
  ) {
    let span = tracing::warn_span!(
      "add_player_enter",
      player_id = self.player_id,
      enter_game_id,
      enter_player_id = player.id
    );
    let _enter = span.enter();

    if let Some(current_game_id) = self.game.current_game_id.clone() {
      if enter_game_id != current_game_id {
        tracing::warn!(
          current_game_id,
          enter_game_id,
          "enter_game_id != current_game_id"
        );
        return;
      }
    }

    self.game.fragments.player_enter.retain(|p| {
      p.slot
        .as_ref()
        .and_then(|slot| {
          let slot_player = slot.player.as_ref()?;
          Some(slot_player.id != player.id)
        })
        .unwrap_or(false)
    });
    self
      .game
      .fragments
      .player_leave
      .retain(|p| p.player_id != player.id);

    self
      .game
      .fragments
      .player_enter
      .push(PacketGamePlayerEnter {
        game_id: enter_game_id,
        slot_index,
        slot: Slot {
          player: Some(player),
          settings: Some(slot_settings),
        }
        .into(),
      });
  }

  pub fn add_player_leave(
    &mut self,
    leave_game_id: i32,
    leave_player_id: i32,
    reason: PlayerLeaveReason,
  ) {
    let span = tracing::warn_span!(
      "add_player_leave",
      player_id = self.player_id,
      leave_game_id,
      leave_player_id,
      reason = i32::from(reason)
    );
    let _enter = span.enter();

    if let Some(current_game_id) = self.game.current_game_id.clone() {
      if leave_game_id != current_game_id {
        tracing::warn!(
          current_game_id,
          leave_game_id,
          "leave_game_id != current_game_id"
        );
        return;
      }
    }

    if leave_player_id == self.player_id {
      self.game.full_game.take();
      self.game.fragments.clear();
    } else {
      self
        .game
        .fragments
        .player_leave
        .retain(|p| p.player_id != leave_player_id);
    }

    self
      .game
      .fragments
      .player_leave
      .push(PacketGamePlayerLeave {
        game_id: leave_game_id,
        player_id: leave_player_id,
        reason: reason.into(),
      });
  }

  pub fn add_slot_update(
    &mut self,
    update_game_id: i32,
    slot_index: i32,
    slot_settings: SlotSettings,
  ) {
    let span = tracing::warn_span!(
      "add_slot_update",
      player_id = self.player_id,
      update_game_id,
      slot_index
    );
    let _enter = span.enter();

    if let Some(current_game_id) = self.game.current_game_id.clone() {
      if update_game_id != current_game_id {
        tracing::warn!(
          current_game_id,
          update_game_id,
          "update_game_id != current_game_id"
        );
        return;
      }
    }

    self
      .game
      .fragments
      .slot_updates
      .retain(|p| p.slot_index != slot_index);
    self.game.fragments.slot_updates.push(PacketGameSlotUpdate {
      game_id: update_game_id,
      slot_index,
      slot_settings: slot_settings.into(),
    })
  }

  pub fn take_frames(&mut self) -> Result<Option<Vec<Frame>>> {
    let len = self.len();
    if len == 0 {
      return Ok(None);
    }

    self.game.current_game_id.take();

    let mut frames = Vec::with_capacity(len);
    if let Some(update) = self.session_update.take() {
      frames.push(update.encode_as_frame()?);
    }

    if let Some(game) = self.game.full_game.take() {
      frames.push(game.encode_as_frame()?);
    }

    for packet in &self.game.fragments.player_enter {
      frames.push(packet.encode_as_frame()?);
    }

    for packet in &self.game.fragments.player_leave {
      frames.push(packet.encode_as_frame()?);
    }

    for packet in &self.game.fragments.slot_updates {
      frames.push(packet.encode_as_frame()?);
    }

    self.game.fragments.clear();
    Ok(Some(frames))
  }

  fn len(&self) -> usize {
    (if self.session_update.is_some() { 1 } else { 0 }) + self.game.len()
  }
}

#[derive(Debug)]
struct GameData {
  current_game_id: Option<i32>,
  full_game: Option<PacketGameInfo>,
  fragments: Fragments,
}

impl GameData {
  fn new(current_game: Option<GameInfo>) -> Self {
    GameData {
      current_game_id: current_game.as_ref().map(|g| g.id),
      full_game: current_game.map(|game| PacketGameInfo { game: Some(game) }),
      fragments: Default::default(),
    }
  }

  fn len(&self) -> usize {
    (if self.full_game.is_some() { 1 } else { 0 }) + self.fragments.len()
  }
}

#[derive(Debug, Default)]
struct Fragments {
  pub player_enter: Vec<PacketGamePlayerEnter>,
  pub player_leave: Vec<PacketGamePlayerLeave>,
  pub slot_updates: Vec<PacketGameSlotUpdate>,
}

impl Fragments {
  fn len(&self) -> usize {
    self.player_enter.len() + self.player_leave.len() + self.slot_updates.len()
  }

  fn clear(&mut self) {
    self.player_enter.clear();
    self.player_leave.clear();
    self.slot_updates.clear();
  }
}
