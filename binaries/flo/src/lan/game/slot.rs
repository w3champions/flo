use flo_w3gs::slot::{SlotData, SlotInfo};

use crate::error::*;
use crate::types::{Slot, SlotStatus};

#[derive(Debug)]
pub struct LanSlotInfo {
  pub slot_player_id: u8,
  pub slot_info: SlotInfo,
  pub my_slot: SlotData,
  pub player_infos: Vec<LanSlotPlayerInfo>,
}

#[derive(Debug)]
pub struct LanSlotPlayerInfo {
  pub slot_player_id: u8,
  pub slot_index: usize,
  pub player_id: i32,
  pub name: String,
}

pub fn build_player_slot_info(
  player_id: i32,
  random_seed: i32,
  slots: &[Slot],
) -> Result<LanSlotInfo> {
  let my_slot_index = slots
    .into_iter()
    .position(|slot| slot.player.as_ref().map(|p| p.id) == Some(player_id))
    .ok_or_else(|| Error::SlotNotResolved)?;

  let player_slots: Vec<(usize, &Slot)> = slots
    .into_iter()
    .enumerate()
    .filter(|(_, slot)| slot.settings.status == SlotStatus::Occupied)
    .collect();

  if player_slots.is_empty() {
    tracing::error!("game has no player slot");
    return Err(Error::SlotNotResolved);
  }

  let slot_player_id = index_to_player_id(my_slot_index);
  let mut slot_info = {
    let mut b = SlotInfo::build();
    b.random_seed(random_seed)
      .num_slots(24)
      .num_players(player_slots.len())
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

  Ok(LanSlotInfo {
    slot_player_id,
    my_slot: slot_info.slots()[my_slot_index].clone(),
    slot_info,
    player_infos,
  })
}

fn index_to_player_id(index: usize) -> u8 {
  return (index + 1) as u8;
}
