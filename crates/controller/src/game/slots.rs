use diesel::helper_types::Nullable;
use diesel::prelude::*;
use std::collections::HashMap;

use crate::game::{Slot, SlotClientStatus, SlotSettings, SlotSettingsColumns, SlotStatus};
use crate::player::{PlayerRef, PlayerRefColumns};
use crate::schema::game_used_slot;

#[derive(Debug)]
pub struct Slots {
  inner: Vec<Slot>,
  map_players: usize,
}

impl Slots {
  pub fn new(map_players: usize) -> Self {
    let inner = std::iter::repeat(())
      .take(24)
      .enumerate()
      .map(|(idx, _)| Self::make_unused_slot(map_players, idx))
      .collect();

    Self { inner, map_players }
  }

  pub fn from_used(map_players: usize, slots: Vec<UsedSlot>) -> Self {
    let mut slot_map: HashMap<_, _> = slots
      .into_iter()
      .map(|slot| (slot.slot_index as usize, slot))
      .collect();
    let inner = std::iter::repeat(())
      .take(24)
      .enumerate()
      .map(|(idx, _)| {
        if let Some(used) = slot_map.remove(&idx) {
          Slot {
            player: used.player,
            settings: used.settings,
            client_status: used.client_status,
          }
        } else {
          Self::make_unused_slot(map_players, idx)
        }
      })
      .collect();
    Slots { map_players, inner }
  }

  pub fn as_used(&self) -> Vec<UsedSlot> {
    self
      .inner
      .iter()
      .enumerate()
      .filter_map(|(index, slot)| {
        if slot.settings.status != SlotStatus::Open {
          Some(UsedSlot::from((index, slot)))
        } else {
          None
        }
      })
      .collect()
  }

  pub fn into_inner(self) -> Vec<Slot> {
    self.inner
  }

  fn make_unused_slot(map_players: usize, idx: usize) -> Slot {
    Slot {
      settings: SlotSettings {
        team: if idx >= map_players {
          24 // Referees
        } else {
          0
        },
        ..Default::default()
      },
      ..Default::default()
    }
  }
}

impl std::ops::Deref for Slots {
  type Target = [Slot];

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl Slots {
  pub fn get_player_ids(&self) -> Vec<i32> {
    self
      .inner
      .iter()
      .filter_map(|slot| slot.player.as_ref().map(|p| p.id))
      .collect()
  }

  pub fn is_full(&self) -> bool {
    !self
      .inner
      .iter()
      .any(|s| s.settings.status == SlotStatus::Open)
  }

  pub fn is_empty(&self) -> bool {
    !self.inner.iter().any(|s| s.player.is_some())
  }

  pub fn join(&mut self, player: &PlayerRef) -> Option<&mut Slot> {
    self.acquire_slot_mut().map(|s| {
      s.player = Some(player.clone());
      s
    })
  }

  pub fn find_player_slot(&self, player_id: i32) -> Option<&Slot> {
    self
      .inner
      .iter()
      .find(|s| s.player.as_ref().map(|p| p.id) == Some(player_id))
  }

  /// Find next open slot, update team, color and status then return it
  pub fn acquire_slot_mut(&mut self) -> Option<&mut Slot> {
    let mut open_slot_idx = None;
    let mut color_set = [false; 24];
    for (i, slot) in self.inner.iter().enumerate() {
      match slot.settings.status {
        SlotStatus::Occupied => {
          if slot.settings.color < 24 {
            color_set[slot.settings.color as usize] = true;
          }
        }
        SlotStatus::Open => {
          if let None = open_slot_idx {
            open_slot_idx = Some(i)
          }
        }
        SlotStatus::Closed => {}
      }
    }
    let mut color = 0;
    for i in 0..24 {
      if !color_set[i] {
        color = i as i32;
        break;
      }
    }

    if let Some(idx) = open_slot_idx {
      let slot = &mut self.inner[idx];
      slot.settings.team = 0;
      slot.settings.color = color as i32;
      slot.settings.status = SlotStatus::Occupied;
      Some(slot)
    } else {
      None
    }
  }

  /// Remove a players and reset the slot
  pub fn release_player_slot(&mut self, player_id: i32) -> bool {
    let slot = self.inner.iter_mut().find(|s| {
      s.player
        .as_ref()
        .map(|p| p.id == player_id)
        .unwrap_or_default()
    });
    match slot {
      Some(slot) => {
        *slot = Default::default();
        true
      }
      None => false,
    }
  }

  /// Remove all players, return removed player ids
  pub fn release_all_player_slots(&mut self) -> Vec<i32> {
    let mut player_ids = vec![];
    for slot in &mut self.inner {
      if let Some(id) = slot.player.as_ref().map(|p| p.id) {
        player_ids.push(id);
        *slot = Default::default();
      }
    }
    player_ids
  }

  /// Validate and update a slot
  pub fn update_slot_at(&mut self, slot_index: i32, settings: &SlotSettings) -> Option<&Slot> {
    let color_set = self.get_color_set();

    if slot_index < 0 || slot_index > 23 {
      return None;
    }

    let slot = &mut self.inner[slot_index as usize];

    let new_color = settings.color;
    if new_color < 24 && slot.settings.color != new_color {
      if !color_set[new_color as usize] {
        slot.settings.color = new_color;
      }
    }

    let new_team = settings.team;
    if new_team != slot.settings.team {
      // referees -> players: reset color
      if slot.settings.team == 24 && new_team != 24 {
        let next_color = color_set.iter().position(|v| !*v).map(|v| v as i32);
        slot.settings.color = next_color.unwrap_or_default();
      }
    }

    if new_team < self.map_players as i32 || new_team == 24 {
      slot.settings.team = new_team;
    }

    let new_handicap = settings.handicap;
    if new_handicap >= 50 && new_handicap <= 100 {
      slot.settings.handicap = new_handicap - (new_handicap % 10);
    }

    slot.settings.race = settings.race;

    Some(&self.inner[slot_index as usize])
  }

  fn get_color_set(&self) -> [bool; 24] {
    let mut set = [false; 24];
    for slot in &self.inner {
      if slot.settings.status == SlotStatus::Occupied && slot.settings.team != 24 {
        if slot.settings.color < 24 {
          set[slot.settings.color as usize] = true;
        }
      }
    }
    return set;
  }
}

#[derive(Debug, Queryable)]
pub struct UsedSlot {
  pub slot_index: i32,
  pub settings: SlotSettings,
  pub client_status: SlotClientStatus,
  pub player: Option<PlayerRef>,
}

impl<'a> From<(usize, &'a Slot)> for UsedSlot {
  fn from((index, slot): (usize, &'a Slot)) -> Self {
    UsedSlot {
      slot_index: index as i32,
      player: slot.player.clone(),
      settings: slot.settings.clone(),
      client_status: slot.client_status,
    }
  }
}

pub(crate) type UsedSlotColumns = (
  game_used_slot::dsl::slot_index,
  SlotSettingsColumns,
  game_used_slot::dsl::client_status,
  Nullable<PlayerRefColumns>,
);

impl UsedSlot {
  pub(crate) fn columns() -> UsedSlotColumns {
    (
      game_used_slot::dsl::slot_index,
      SlotSettings::COLUMNS,
      game_used_slot::dsl::client_status,
      PlayerRef::COLUMNS.nullable(),
    )
  }
}
