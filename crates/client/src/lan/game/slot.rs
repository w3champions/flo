use flo_w3gs::slot::{RacePref, SlotData, SlotInfo};

use crate::error::*;
use flo_types::game::{LanGameSlot, SlotStatus};

#[derive(Debug)]
pub struct LanSlotInfo {
  pub my_slot_player_id: u8,
  pub slot_info: SlotInfo,
  pub my_slot: SlotData,
  pub player_infos: Vec<LanSlotPlayerInfo>,
  pub stream_ob_slot: Option<usize>,
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

pub fn build_player_slot_info<'a, P, S>(
  self_player: P,
  random_seed: i32,
  slots: &'a [S],
) -> Result<LanSlotInfo>
where
  P: Into<SelfPlayer>,
  S: 'a,
  &'a S: Into<LanGameSlot<'a>>,
{
  const FLO_OB_SLOT: usize = 23;
  let self_player: SelfPlayer = self_player.into();
  let slots: Vec<LanGameSlot> = slots.into_iter().map(Into::into).collect();

  let occupied_slots: Vec<(usize, _)> = slots
    .iter()
    .enumerate()
    .filter(|(_, slot)| slot.settings.status == SlotStatus::Occupied)
    .collect();

  if occupied_slots.is_empty() {
    tracing::error!("game has no player slot");
    return Err(Error::SlotNotResolved);
  }

  let flo_ob_slot_occupied = occupied_slots
    .iter()
    .find(|(idx, _)| *idx == FLO_OB_SLOT)
    .is_some();

  let stream_ob_slot = if let SelfPlayer::StreamObserver = self_player {
    if occupied_slots.len() > 23 {
      return Err(Error::FloObserverSlotOccupied);
    }
    Some(FLO_OB_SLOT)
  } else {
    if flo_ob_slot_occupied {
      None
    } else {
      Some(FLO_OB_SLOT)
    }
  };

  let mut slot_info = {
    let mut b = SlotInfo::build();
    b.random_seed(random_seed)
      .num_slots(24)
      .num_players(
        occupied_slots
          .iter()
          .filter(|(_, slot)| slot.settings.team != 24)
          .count(),
      )
      .build()
  };

  for (i, player_slot) in &occupied_slots {
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

  if let Some(ob_slot_idx) = stream_ob_slot.clone() {
    use flo_w3gs::slot::SlotStatus;
    let slot = slot_info
      .slot_mut(ob_slot_idx)
      .expect("always has 24 slots");

    slot.player_id = index_to_player_id(ob_slot_idx);
    slot.slot_status = SlotStatus::Occupied;
    slot.race = RacePref::RANDOM;
    slot.color = 0;
    slot.team = 24;
  };

  let player_infos = occupied_slots
    .into_iter()
    .filter_map(|(i, slot)| {
      if stream_ob_slot == Some(i) {
        return None;
      }

      if let Some(player) = slot.player.as_ref() {
        Some(LanSlotPlayerInfo {
          slot_player_id: index_to_player_id(i),
          slot_index: i,
          player_id: player.id,
          name: player.name.to_string(),
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
    SelfPlayer::StreamObserver => stream_ob_slot
      .clone()
      .ok_or_else(|| Error::SlotNotResolved)?,
  };

  let my_slot_player_id = index_to_player_id(my_slot_index);

  Ok(LanSlotInfo {
    my_slot_player_id,
    my_slot: slot_info.slots()[my_slot_index].clone(),
    slot_info,
    player_infos,
    stream_ob_slot,
  })
}

pub fn index_to_player_id(index: usize) -> u8 {
  return (index + 1) as u8;
}
