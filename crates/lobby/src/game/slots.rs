use serde::{Deserialize, Serialize};

use crate::game::{Computer, Race, Slot, SlotSettings, SlotStatus};
use crate::player::PlayerRef;

#[derive(Debug)]
pub struct Slots {
  inner: Vec<Slot>,
  num_of_players: usize,
}

impl Slots {
  pub fn new(num_of_players: usize) -> Self {
    let inner = std::iter::repeat(())
      .take(24)
      .enumerate()
      .map(|(idx, _)| {
        Slot {
          player: None,
          settings: SlotSettings {
            team: if idx >= num_of_players {
              24 // Referees
            } else {
              0
            },
            color: 0,
            computer: Computer::Easy,
            handicap: 100,
            status: SlotStatus::Open,
            race: Race::Human,
          },
        }
      })
      .collect();

    Self {
      inner,
      num_of_players,
    }
  }

  pub fn from_vec(slots: Vec<Slot>) -> Self {
    Slots {
      num_of_players: slots.iter().filter(|s| s.settings.team != 24).count(),
      inner: slots,
    }
  }

  pub fn into_inner(self) -> Vec<Slot> {
    self.inner
  }
}

impl std::ops::Deref for Slots {
  type Target = [Slot];

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl Slots {
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
      slot.settings.team = 1;
      slot.settings.color = color as u32;
      slot.settings.status = SlotStatus::Occupied;
      Some(slot)
    } else {
      None
    }
  }

  /// Remove a players and reset the slot
  pub fn release_player_slot(&mut self, player_id: i32) -> bool {
    let slot = self
      .inner
      .iter_mut()
      .find(|s| s.player.as_ref().map(|p| p.id == player_id).is_some());
    match slot {
      Some(slot) => {
        *slot = Default::default();
        true
      }
      None => false,
    }
  }

  /// Find a player's slot, do some validation and update it
  pub fn update_player_slot(&mut self, player_id: i32, settings: &SlotSettings) -> bool {
    let color_set = self.get_color_set();

    let slot = self
      .inner
      .iter_mut()
      .find(|s| s.player.as_ref().map(|p| p.id == player_id).is_some());

    match slot {
      Some(slot) => {
        let new_color = settings.color;
        if new_color < 24 && slot.settings.color != new_color {
          if !color_set[new_color as usize] {
            slot.settings.color = new_color;
          }
        }

        let new_team = settings.team;
        if new_team < self.num_of_players as u32 || new_team == 24 {
          slot.settings.team = new_team;
        }

        let new_handicap = settings.handicap;
        if new_handicap >= 50 && new_handicap <= 100 {
          slot.settings.handicap = new_handicap - (new_handicap % 10);
        }

        slot.settings.race = settings.race;

        true
      }
      None => false,
    }
  }

  fn get_color_set(&self) -> [bool; 24] {
    let mut set = [false; 24];
    for slot in &self.inner {
      if let SlotStatus::Occupied = slot.settings.status {
        if slot.settings.color < 24 {
          set[slot.settings.color as usize] = true;
        }
      }
    }
    return set;
  }
}

#[test]
fn test_slot_ops() {
  let mut slots = Slots::new(4);
}
