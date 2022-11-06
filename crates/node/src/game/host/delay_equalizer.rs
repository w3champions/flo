//! Dynamically calculate the value of artificial delay that should be added to each player,
//! so that the players' delays are approximately the same

use std::collections::BTreeMap;

/// Max value of rtt + delay
const MAX_RTT: u32 = 200;

/// Only up to this much delay is allowed to be added
const MAX_ADDED_DELAY_MS: u32 = 150;

/// Smoothen RTT adjustment
const RTT_ROLLING_SAMPLE_SIZE: usize = 60;

/// Only start adjust pings after 5 samples have been obtained
const MIN_SAMPLES: usize = 5;

#[derive(Debug)]
pub struct DelayEqualizer<const N: usize = RTT_ROLLING_SAMPLE_SIZE> {
  slots: BTreeMap<i32, Slot<N>>,
  max_player_count: usize,
  ready: bool,
  top: Option<Top>,
}

impl<const N: usize> DelayEqualizer<N> {
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
    let real_rtt = {
      let slot = self
        .slots
        .entry(player_id)
        .or_insert_with(|| Slot::default());
      if slot.removed {
        return None;
      }
      let real_rtt = rtt.saturating_sub(slot.delay.clone().unwrap_or_default());
      slot.update_real_rtt(real_rtt)
    };

    // Update top rtt
    let is_top = if let Some(ref mut top) = self.top {
      if top.player_id == player_id {
        top.real_rtt = real_rtt;
        true
      } else {
        false
      }
    } else {
      false
    };

    if let Some(top) = self.check_ready().cloned() {
      if let Some(slot) = self.slots.get_mut(&player_id) {
        if is_top {
          // Top should always have 0 delay
          if slot.delay.clone().unwrap_or_default() > 0 {
            slot.try_update_delay(0)
          } else {
            None
          }
        } else {
          return if real_rtt > top.real_rtt {
            // Found a new top
            self.top.replace(Top {
              player_id,
              real_rtt,
            });
            // Remove delay for the new top
            // Old top will be updated on its next `insert_rtt` call
            slot.try_update_delay(0)
          } else {
            let new_delay = get_new_delay_value(slot.delay.clone(), top.real_rtt, real_rtt);
            slot.try_update_delay(new_delay)
          };
        }
      } else {
        None
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
      for (_, slot) in &self.slots {
        if !slot.removed && slot.count < MIN_SAMPLES {
          return None;
        }
      }
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
      .max_by_key(|(_, slot)| slot.rtt_comparable())
      .map(|(player_id, slot)| Top {
        player_id: *player_id,
        real_rtt: slot
          .rolling_average
          .clone()
          .map(|v| v as u32)
          .unwrap_or(MAX_RTT),
      })
  }
}

fn get_new_delay_value(current_delay: Option<u32>, top_rtt: u32, rtt: u32) -> u32 {
  let mut rtt_diff = top_rtt.saturating_sub(rtt);
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
    if delay_diff > 0 {
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
struct Slot<const N: usize> {
  removed: bool,
  rolling_average: Option<f32>,
  delay: Option<u32>,
  count: usize,
}

impl<const N: usize> Slot<N> {
  fn update_real_rtt(&mut self, new_real_rtt: u32) -> u32 {
    self.count = std::cmp::min(N, self.count + 1);
    let v = if let Some(v) = self.rolling_average.clone() {
      let rolling_average = v * ((self.count - 1) as f32) / (self.count as f32)
        + (new_real_rtt as f32) / (self.count as f32);
      let v = if rolling_average < u32::MAX as f32 {
        rolling_average as u32
      } else {
        u32::MAX
      };
      self.rolling_average.replace(rolling_average);
      v
    } else {
      self.rolling_average.replace(new_real_rtt as f32);
      new_real_rtt
    };
    v
  }

  fn rtt_comparable(&self) -> Option<u32> {
    self.rolling_average.clone().map(|v| v as u32)
  }

  fn try_update_delay(&mut self, new_delay: u32) -> Option<u32> {
    let delay = self.delay.clone().unwrap_or_default();

    if delay != new_delay && new_delay == 0 {
      self.delay.replace(0);
      return Some(0);
    }

    let delay_diff = if delay > new_delay {
      delay - new_delay
    } else {
      new_delay - delay
    };
    if delay_diff > 0 {
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
  real_rtt: u32,
}

#[test]
fn test_get_new_delay_value() {
  let v = get_new_delay_value(None, 100, 10);
  assert_eq!(v, 100 - 10);

  // MAX_DELAY
  let v = get_new_delay_value(None, MAX_ADDED_DELAY_MS + 100, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(MAX_ADDED_DELAY_MS), MAX_ADDED_DELAY_MS + 100, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(0), MAX_ADDED_DELAY_MS + 100, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);
  let v = get_new_delay_value(Some(1000), MAX_ADDED_DELAY_MS + 100, 10);
  assert_eq!(v, MAX_ADDED_DELAY_MS);

  // MAX_RTT
  let v = get_new_delay_value(None, MAX_RTT + 300, 110);
  assert_eq!(v, MAX_RTT - 110);
  let v = get_new_delay_value(Some(0), MAX_RTT + 300, 110);
  assert_eq!(v, MAX_RTT - 110);
  let v = get_new_delay_value(Some(1000), MAX_RTT + 300, 110);
  assert_eq!(v, MAX_RTT - 110);
}

#[test]
fn test_delay_equalizer() {
  let mut e = DelayEqualizer::<1>::new(2);
  assert!(e.insert_rtt(1, 10).is_none());
  assert!(e.insert_rtt(1, 100).is_none());
  assert!(e.check_ready().is_none());

  assert_eq!(e.insert_rtt(2, 10), Some(100 - 10)); // 1 = TOP = 100, 2 = 10+90
  assert!(e.check_ready().is_some());

  assert_eq!(e.top.clone().unwrap().player_id, 1);

  assert_eq!(e.insert_rtt(2, 60 + 90), Some(40)); // 2 <- 150-90=60, 2 = +40 ==> 1 = TOP = 100, 2 = 60+40
  assert_eq!(e.top.clone().unwrap().player_id, 1);

  assert_eq!(e.insert_rtt(2, 40 + 110), Some(0)); // 2 <- 150-40=110 ==> 1 = 100, 2 = TOP = 110
  assert_eq!(e.top.clone().unwrap().player_id, 2);

  assert_eq!(e.insert_rtt(1, 100), Some(10)); // 1 = 100+10, 2 = TOP = 110

  assert_eq!(e.insert_rtt(2, 10), None); // 1 = 100+10, 2 = 10
  assert_eq!(e.insert_rtt(1, 110), Some(0)); // 1 = TOP = 100, 2 = 10
  assert_eq!(e.top.clone().unwrap().player_id, 1);

  assert_eq!(e.insert_rtt(2, 10), Some(90)); // 1 = TOP = 100, 2 = 10 + 90
}
#[test]
fn test_delay_equalizer_fuzzy() {
  use rand::distributions::{Open01, Slice};
  use rand::{thread_rng, Rng};

  const SIMPLE_SIZE: usize = 1000;
  const PLAYER_COUNT: usize = 2;
  const BASE_RTT: [u32; PLAYER_COUNT] = [10, 60];
  const MAX_SPIKE: [f32; PLAYER_COUNT] = [100., 100.];
  const SPIKE_RATE: [f32; PLAYER_COUNT] = [0.5, 0.05];
  let mut e: DelayEqualizer<1> = DelayEqualizer::new(PLAYER_COUNT);
  let mut current: [u32; PLAYER_COUNT] = [0, 0];
  let mut delay: [u32; PLAYER_COUNT] = [0, 0];
  for s in 0..SIMPLE_SIZE {
    for i in 0..PLAYER_COUNT {
      let r: f32 = thread_rng().sample(Open01);
      let spike = if r < SPIKE_RATE[i] {
        MAX_SPIKE[i] * thread_rng().sample(Slice::new(&[0.3, 0.5, 1.0]).unwrap())
      } else {
        0_f32
      };
      let real_rtt = BASE_RTT[i] + spike as u32;
      let rtt = real_rtt + delay[i];
      if let Some(d) = e.insert_rtt(i as i32, rtt) {
        delay[i] = d;
      }
      current[i] = real_rtt + delay[i];
    }
    let [a, b] = current;
    if a != b {
      println!("!#{s}: {a} != {b}");
      for i in 0..PLAYER_COUNT {
        println!("- {i}: rtt = {}, delay = {}", current[i], delay[i]);
      }
    } else {
      println!("#{s}: {a} = {b}");
    }
  }
}
