use flo_w3gs::slot::{RacePref, SlotData, SlotInfo};

use crate::error::*;
use crate::types::{Slot, SlotStatus};

#[derive(Debug)]
pub struct LanSlotInfo {
  pub my_slot_player_id: u8,
  pub slot_info: SlotInfo,
  pub my_slot: SlotData,
  pub player_infos: Vec<LanSlotPlayerInfo>,
  pub has_stream_obs_slot: bool,
}

#[derive(Debug)]
pub struct LanSlotPlayerInfo {
  pub slot_player_id: u8,
  pub slot_index: usize,
  pub player_id: i32,
  pub name: String,
}

pub enum SelfPlayer {
  Player(i32),
  StreamObserver,
}

impl From<i32> for SelfPlayer {
  fn from(id: i32) -> Self {
    Self::Player(id)
  }
}

pub fn build_player_slot_info<S: Into<SelfPlayer>>(
  self_player: S,
  random_seed: i32,
  slots: &[Slot],
) -> Result<LanSlotInfo> {
  let self_player: SelfPlayer = self_player.into();

  let player_slots: Vec<(usize, &Slot)> = slots
    .into_iter()
    .enumerate()
    .filter(|(_, slot)| slot.settings.status == SlotStatus::Occupied)
    .collect();

  if player_slots.is_empty() {
    tracing::error!("game has no player slot");
    return Err(Error::SlotNotResolved);
  }

  let has_stream_obs_slot = if let SelfPlayer::StreamObserver = self_player {
    if player_slots.len() > 23 {
      return Err(Error::NoVacantSlotForObserver);
    }
    true
  } else {
    false
  };

  let mut slot_info = {
    let mut b = SlotInfo::build();
    b.random_seed(random_seed)
      .num_slots(24)
      .num_players(
        player_slots
          .iter()
          .filter(|(_, slot)| slot.settings.team != 24)
          .count()
          + if has_stream_obs_slot { 1 } else { 0 },
      )
      .build()
  };

  for (i, player_slot) in &player_slots {
    use flo_w3gs::slot::SlotStatus;
    let slot = slot_info.slot_mut(*i).expect("always has 24 slots");

    if player_slot.player.is_some() {
      slot.player_id = index_to_player_id(*i);
      slot.slot_status = SlotStatus::Occupied;
      slot.race = player_slot.settings.race.into();
      slot.color = player_slot.settings.color as u8;
      slot.team = player_slot.settings.team as u8;
      slot.handicap = player_slot.settings.handicap as u8;
      slot.download_status = 100;
    } else {
      slot.computer = true;
      slot.computer_type = player_slot.settings.computer.into();
      slot.slot_status = SlotStatus::Occupied;
      slot.race = player_slot.settings.race.into();
      slot.color = player_slot.settings.color as u8;
      slot.team = player_slot.settings.team as u8;
      slot.handicap = player_slot.settings.handicap as u8;
      slot.download_status = 100;
    }
  }

  if has_stream_obs_slot {
    use flo_w3gs::slot::SlotStatus;
    let idx = player_slots.len();
    let slot = slot_info.slot_mut(idx).expect("always has 24 slots");
    slot.player_id = index_to_player_id(idx);
    slot.slot_status = SlotStatus::Occupied;
    slot.race = RacePref::RANDOM;
    slot.color = 0;
    slot.team = 24;
    slot.handicap = 100;
    slot.download_status = 100;
  };

  let player_infos = player_slots
    .into_iter()
    .filter_map(|(i, slot)| {
      if let Some(player) = slot.player.as_ref() {
        Some(LanSlotPlayerInfo {
          slot_player_id: index_to_player_id(i),
          slot_index: i,
          player_id: player.id,
          name: player.name.clone(),
        })
      } else {
        None
      }
    })
    .collect();

  let my_slot_index = match self_player {
    SelfPlayer::Player(player_id) => slots
      .into_iter()
      .position(|slot| slot.player.as_ref().map(|p| p.id) == Some(player_id))
      .ok_or_else(|| Error::SlotNotResolved)?,
    SelfPlayer::StreamObserver => slots.len(),
  };

  let my_slot_player_id = index_to_player_id(my_slot_index);

  Ok(LanSlotInfo {
    my_slot_player_id,
    my_slot: slot_info.slots()[my_slot_index].clone(),
    slot_info,
    player_infos,
    has_stream_obs_slot,
  })
}

pub fn index_to_player_id(index: usize) -> u8 {
  return (index + 1) as u8;
}
