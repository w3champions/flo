//! Dynamically calculate the value of artificial delay that should be added to each player,
//! so that the players' delays are approximately the same

use std::collections::BTreeMap;

use once_cell::sync::Lazy;

/// Max value of rtt + delay
const MAX_RTT: u32 = 150;

/// Only up to this much delay is allowed to be added
const MAX_ADDED_DELAY_MS: u32 = 100;

/// Retain a certain latency advantage to avoid dynamically
/// added latency resulting in higher actual latency for players
/// with fast internet speeds
const HOME_ADVANTAGE_MS: Lazy<u32> = Lazy::new(|| {
  std::env::var("FLO_HOME_ADVANTAGE_MS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(5)
});

/// The delay is adjusted only if the difference between the new value and the old value is
/// greater than this value. (`DelayedFrameStream::set_delay` is expensive)
const MIN_ADJUST_DIFFERENCE: u32 = 10;

#[derive(Debug)]
pub struct DelayEqualizer {
  slots: BTreeMap<i32, Slot>,
  max_player_count: usize,
  ready: bool,
  top: Option<Top>,
}

impl DelayEqualizer {
  pub fn new(player_count: usize) -> Self {
    Self {
      slots: BTreeMap::new(),
      max_player_count: player_count,
      ready: false,
      top: None,
    }
  }

  /// Record the player's latest rtt, and if the players' delay needs to be adjusted, return the new delay
  pub fn insert_rtt(&mut self, player_id: i32, rtt: u32) -> Option<u32> {
    {
      let slot = self
        .slots
        .entry(player_id)
        .or_insert_with(|| Slot::default());
      if slot.removed {
        return None;
      }
      slot.rtt.replace(rtt);
    }

    if let Some(top) = self.check_ready().cloned() {
      if top.player_id == player_id {
        // Top's rtt has changed, just record the new rtt.
        None
      } else {
        if let Some(slot) = self.slots.get_mut(&player_id) {
          return if rtt > top.rtt && rtt - top.rtt > MIN_ADJUST_DIFFERENCE {
            // Found a new top
            self.top.replace(Top { player_id, rtt });
            // Remove delay for the new top
            // Old top will be updated on its next `insert_rtt` call
            slot.try_update_delay(0)
          } else {
            let new_delay = get_new_delay_value(
              slot.delay.clone(),
              top.rtt,
              rtt.saturating_sub(slot.delay.clone().unwrap_or_default()),
            );
            slot.try_update_delay(new_delay)
          };
        } else {
          None
        }
      }
    } else {
      None
    }
  }

  pub fn remove_player(&mut self, player_id: i32) {
    self
      .slots
      .entry(player_id)
      .or_insert_with(|| Slot::default())
      .removed = true;
    if let Some(top_player_id) = self.check_ready().map(|v| v.player_id) {
      // top gone, find another top
      if top_player_id == player_id {
        self.top = self.find_top();
      }
    }
  }

  fn check_ready(&mut self) -> Option<&Top> {
    if self.ready {
      return self.top.as_ref();
    }
    if self.max_player_count == self.slots.len() {
      self.ready = true;
      self.top = self.find_top();
      return self.top.as_ref();
    }
    None
  }

  fn find_top(&self) -> Option<Top> {
    self
      .slots
      .iter()
      .filter(|(_, slot)| !slot.removed)
      .max_by_key(|(_, slot)| slot.rtt)
      .map(|(player_id, slot)| Top {
        player_id: *player_id,
        rtt: slot.rtt.clone().unwrap_or(MAX_RTT),
      })
  }
}

fn get_new_delay_value(current_delay: Option<u32>, top_rtt: u32, rtt: u32) -> u32 {
  let mut rtt_diff = top_rtt
    .saturating_sub(rtt)
    .saturating_sub(*HOME_ADVANTAGE_MS);
  if rtt_diff == 0 {
    return 0;
  }

  // delay should <= MAX_ADDED_DELAY_MS
  if rtt_diff > MAX_ADDED_DELAY_MS {
    rtt_diff = MAX_ADDED_DELAY_MS;
  }

  let mut delay = if let Some(current_delay) = current_delay {
    let delay_diff = if current_delay > rtt_diff {
      current_delay - rtt_diff
    } else {
      rtt_diff - current_delay
    };
    if delay_diff >= MIN_ADJUST_DIFFERENCE {
      rtt_diff
    } else {
      current_delay
    }
  } else {
    rtt_diff
  };

  // rtt + delay should <= MAX_RTT
  if rtt + delay > MAX_RTT {
    delay = MAX_RTT.saturating_sub(rtt);
  }
  delay
}

#[derive(Debug, Default)]
struct Slot {
  removed: bool,
  rtt: Option<u32>,
  delay: Option<u32>,
}

impl Slot {
  fn try_update_delay(&mut self, new_delay: u32) -> Option<u32> {
    let delay = self.delay.clone().unwrap_or_default();
    let delay_diff = if delay > new_delay {
      delay - new_delay
    } else {
      new_delay - delay
    };
    if delay_diff > MIN_ADJUST_DIFFERENCE {
      self.delay.replace(new_delay);
      Some(new_delay)
    } else {
      None
    }
  }
}

#[derive(Debug, Clone)]
struct Top {
  player_id: i32,
  rtt: u32,
}

#[test]
fn test_get_new_delay_value() {
  let v = get_new_delay_value(None, 100, 10);
  assert_eq!(v, 100 - 10 - HOME_ADVANTAGE_MS);

  // diff >= MIN_ADJUST_DIFFERENCE
  let v = get_new_delay_value(Some(79 - HOME_ADVANTAGE_MS /* 64 */), 100, 10);
  assert_eq!(v, 100 - 10 - HOME_ADVANTAGE_MS);

  // diff < MIN_ADJUST_DIFFERENCE
  let v = get_new_delay_value(Some(85 - HOME_ADVANTAGE_MS), 100, 10);
  assert_eq!(v, 85 - HOME_ADVANTAGE_MS);

  // MAX_DELAY
  let v = get_new_delay_value(None, 130, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(MAX_ADDED_DELAY_MS), 130, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(0), 130, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(1000), 130, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);

  // MAX_RTT
  let v = get_new_delay_value(None, 300, 110);
  assert_eq!(v, MAX_RTT - 110);
  let v = get_new_delay_value(Some(0), 300, 110);
  assert_eq!(v, MAX_RTT - 110);
  let v = get_new_delay_value(Some(1000), 300, 110);
  assert_eq!(v, MAX_RTT - 110);
}

#[test]
fn test_delay_equalizer() {
  let mut e = DelayEqualizer::new(2);
  assert!(e.insert_rtt(1, 10).is_none());
  assert!(e.insert_rtt(1, 100).is_none());
  assert!(e.check_ready().is_none());

  assert_eq!(e.insert_rtt(2, 10), Some(100 - 10 - HOME_ADVANTAGE_MS));
  assert!(e.check_ready().is_some());

  assert_eq!(e.top.clone().unwrap().player_id, 1);

  assert_eq!(e.insert_rtt(2, 10), None);
  assert_eq!(e.insert_rtt(2, 15), None);
  assert_eq!(e.insert_rtt(2, 5), None);

  assert_eq!(e.insert_rtt(2, 150), Some(0));
  assert_eq!(e.top.clone().unwrap().player_id, 2);
  assert_eq!(e.insert_rtt(2, 150), None);
  assert_eq!(e.insert_rtt(1, 100), Some(150 - 100 - HOME_ADVANTAGE_MS));
}
