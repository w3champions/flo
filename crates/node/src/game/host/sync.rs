use slab::Slab;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug)]
pub struct SyncMap {
  tick: u32,
  time: u32,
  players: BTreeMap<i32, PlayerState>,
  pending_tick: BTreeMap<u32, usize>,
  pending_slab: Slab<Pending>,
  desync_buf: Vec<PlayerDesync>,
}

impl SyncMap {
  pub fn new(players: Vec<i32>) -> Self {
    Self {
      tick: 0,
      time: 0,
      players: players
        .into_iter()
        .map(|player_id| (player_id, PlayerState::new()))
        .collect(),
      pending_tick: BTreeMap::new(),
      pending_slab: Slab::new(),
      desync_buf: vec![],
    }
  }

  pub fn time(&self) -> u32 {
    self.time
  }

  #[must_use]
  pub fn clock(&mut self, time_increment: u16) -> Option<Vec<PlayerTimeout>> {
    let timeout_players = self.check_timeout(time_increment);
    let id = self.pending_slab.insert(Pending::new(self.tick, self.time));
    if timeout_players.is_none() {
      self.tick += 1;
      self.time += time_increment as u32;
      self.pending_tick.insert(self.tick, id);
    }
    timeout_players
  }

  fn check_timeout(&mut self, time_increment: u16) -> Option<Vec<PlayerTimeout>> {
    for id in self.pending_tick.values() {
      let item = &mut self.pending_slab[*id];
      if (self.time + time_increment as u32) - item.time
        > crate::constants::GAME_PLAYER_LAGGING_THRESHOLD_MS
      {
        return Some(
          self
            .players
            .keys()
            .filter_map(|player_id| {
              if !item.checksums.contains_key(player_id) {
                Some(PlayerTimeout {
                  player_id: *player_id,
                })
              } else {
                None
              }
            })
            .collect(),
        );
      } else {
        break;
      }
    }
    None
  }

  #[must_use]
  pub fn remove_player(&mut self, player_id: i32) -> Option<Vec<PlayerDesync>> {
    self.players.remove(&player_id);
    self.desync_buf.clear();
    let mut finished = None;
    for (tick, id) in &self.pending_tick {
      let item = &mut self.pending_slab[*id];
      item.checksums.remove(&player_id);
      if let Some(token) = item.should_check_desync(self.players.len()) {
        finished.get_or_insert_with(|| vec![]).push((*tick, *id));
        item.check_desync(token, &mut self.desync_buf);
      }
    }
    if let Some(finished) = finished {
      for (tick, id) in finished {
        self.pending_tick.remove(&tick);
        self.pending_slab.remove(id);
      }
    }
    self.take_desync()
  }

  pub fn player_pending_ticks(&self, player_id: i32) -> Option<u32> {
    self
      .players
      .get(&player_id)
      .map(|v| self.tick.saturating_sub(v.tick))
  }

  pub fn ack(&mut self, player_id: i32, checksum: u32) -> Result<AckResult, AckError> {
    let state = self
      .players
      .get_mut(&player_id)
      .ok_or_else(|| AckError::PlayerNotFound(player_id))?;
    let tick = state.tick + 1;
    state.tick = tick;
    let id = self
      .pending_tick
      .get(&tick)
      .cloned()
      .ok_or_else(|| AckError::TickNotFound(tick))?;
    let pending = &mut self.pending_slab[id];
    state.time = pending.time;
    pending.checksums.insert(player_id, checksum);
    let rtt = Instant::now().saturating_duration_since(pending.t);
    if let Some(token) = pending.should_check_desync(self.players.len()) {
      self.desync_buf.clear();
      pending.check_desync(token, &mut self.desync_buf);
      self.pending_tick.remove(&tick);
      self.pending_slab.remove(id);
      if self.desync_buf.is_empty() {
        Ok(AckResult {
          player_tick: tick,
          game_tick: self.tick,
          rtt,
          desync: None,
        })
      } else {
        Ok(AckResult {
          player_tick: tick,
          game_tick: self.tick,
          rtt,
          desync: self.take_desync(),
        })
      }
    } else {
      Ok(AckResult {
        player_tick: tick,
        game_tick: self.tick,
        rtt,
        desync: None,
      })
    }
  }

  pub fn debug_pending(&self) -> String {
    let mut values = Vec::with_capacity(self.pending_tick.len());
    for (tick, id) in &self.pending_tick {
      let item = &self.pending_slab[*id];
      values.push(format!(
        "- tick={}, time={}, checksums={:?}",
        tick, item.time, item.checksums
      ))
    }
    "debug_pending\n".to_string() + &values.join("\n")
  }

  fn take_desync(&mut self) -> Option<Vec<PlayerDesync>> {
    if self.desync_buf.is_empty() {
      return None;
    }
    std::mem::replace(&mut self.desync_buf, vec![]).into()
  }
}

pub struct AckResult {
  pub game_tick: u32,
  pub player_tick: u32,
  pub rtt: Duration,
  pub desync: Option<Vec<PlayerDesync>>,
}

#[derive(Error, Debug)]
pub enum AckError {
  #[error("player not found: {0}")]
  PlayerNotFound(i32),
  #[error("tick not found: {0}")]
  TickNotFound(u32),
}

#[derive(Debug)]
struct Pending {
  tick: u32,
  time: u32,
  checksums: BTreeMap<i32, u32>,
  t: Instant,
}

impl Pending {
  fn new(tick: u32, time: u32) -> Self {
    Self {
      tick,
      time,
      checksums: BTreeMap::new(),
      t: Instant::now(),
    }
  }

  fn should_check_desync(&self, player_count: usize) -> Option<CheckDesyncToken> {
    if player_count == 0 {
      return None;
    }

    if self.checksums.len() >= player_count {
      Some(CheckDesyncToken)
    } else {
      None
    }
  }

  fn check_desync(&self, _token: CheckDesyncToken, out: &mut Vec<PlayerDesync>) {
    let mut last_checksum = None;
    let mut desync_detected = false;
    for checksum in self.checksums.values() {
      match last_checksum {
        Some(value) => {
          if value != *checksum {
            desync_detected = true;
            break;
          }
        }
        None => {
          last_checksum.replace(*checksum);
        }
      }
    }
    if desync_detected {
      let tick = self.tick;
      let time = self.time;
      let mut vote_map = BTreeMap::new();
      self
        .checksums
        .iter()
        .fold(&mut vote_map, |map, (player_id, checksum)| {
          map
            .entry(*checksum)
            .or_insert_with(|| vec![])
            .push(*player_id);
          map
        });

      let mut votes_sorted: Vec<_> = vote_map.into_iter().collect();
      votes_sorted.sort_by_key(|(_, players)| players.len());
      match votes_sorted.len() {
        0 | 1 => unreachable!(),
        2 => {
          if votes_sorted[0].1.len() == votes_sorted[1].1.len() {
            out.extend(votes_sorted.into_iter().flat_map(|(checksum, players)| {
              players.into_iter().map(move |player_id| PlayerDesync {
                tick,
                time,
                player_id,
                checksum,
              })
            }));
          } else {
            let (checksum, players) = votes_sorted.remove(0);
            out.extend(players.into_iter().map(|player_id| PlayerDesync {
              tick,
              time,
              player_id,
              checksum,
            }));
          }
        }
        n => {
          let last = &votes_sorted[n - 1];
          let second_last = &votes_sorted[n - 2];
          if last.1.len() == second_last.1.len() {
            out.extend(votes_sorted.into_iter().flat_map(|(checksum, players)| {
              players.into_iter().map(move |player_id| PlayerDesync {
                tick,
                time,
                player_id,
                checksum,
              })
            }));
          } else {
            out.extend(
              votes_sorted
                .into_iter()
                .take(n - 1)
                .flat_map(|(checksum, players)| {
                  players.into_iter().map(move |player_id| PlayerDesync {
                    tick,
                    time,
                    player_id,
                    checksum,
                  })
                }),
            )
          }
        }
      }
    }
  }
}

#[must_use]
struct CheckDesyncToken;

#[derive(Debug)]
pub struct PlayerState {
  tick: u32,
  time: u32,
}

impl PlayerState {
  fn new() -> Self {
    Self { tick: 0, time: 0 }
  }
}

#[derive(Debug, PartialOrd, PartialEq)]
pub struct PlayerTimeout {
  pub player_id: i32,
}

#[derive(Debug, PartialOrd, PartialEq)]
pub struct PlayerDesync {
  pub player_id: i32,
  pub tick: u32,
  pub time: u32,
  pub checksum: u32,
}

#[test]
fn test_sync_map() {
  use rand::seq::SliceRandom;
  use rand::{thread_rng, Rng};
  use std::collections::VecDeque;

  const SIZE: u32 = 100000;

  let mut rng = thread_rng();
  let mut players = vec![0, 1, 2, 3];
  let mut map = SyncMap::new(players.clone());

  let mut acks = VecDeque::new();
  let mut buckets = vec![SIZE, SIZE, SIZE, SIZE];
  let drop_player = players.choose(&mut rng).cloned().unwrap();

  dbg!(drop_player);

  loop {
    let player_id = if let Some(player_id) = players.choose(&mut rng) {
      *player_id
    } else {
      break;
    };

    if buckets[player_id as usize] == 0 {
      players.retain(|v| *v != player_id);
      continue;
    }

    let tick: u32 = buckets[player_id as usize];
    buckets[player_id as usize] -= 1;
    let end = buckets.iter().all(|v| *v <= tick);

    // simulate dropping player
    if (player_id == drop_player && buckets[player_id as usize] < SIZE / 2) && !end {
      players.retain(|v| *v != player_id);
      buckets[player_id as usize] = 0;
      continue;
    }

    acks.push_back((SIZE - tick + 1, player_id, end));
  }

  let threshold_ticks = crate::constants::GAME_PLAYER_LAGGING_THRESHOLD_MS / 5;
  let mut offset = 0;
  for tick in 0..(SIZE + 1/* timeout uss 1 iteration */) {
    let timeout = map.clock(5);
    if let Some(items) = timeout {
      assert_eq!(items.len(), 1);
      assert_eq!(items[0].player_id, drop_player);
      let desync = map.remove_player(drop_player);
      dbg!(&desync);
      assert!(desync.is_none());
      offset = 1;
    }

    for _ in 0..rng.gen_range(0..8) {
      if let Some((ack_tick, player_id, end)) = acks.pop_front() {
        if ack_tick <= tick {
          let desync = map.ack(player_id, ack_tick).unwrap().desync;
          if desync.is_some() {
            dbg!(&desync);
          }
          assert!(desync.is_none());
        } else {
          acks.push_front((ack_tick, player_id, end));
          break;
        }
      } else {
        unreachable!()
      }
    }

    let oldest_tick = map.pending_tick.keys().next().cloned().unwrap();
    if oldest_tick + threshold_ticks >= tick {
      let mut deferred = vec![];
      while !acks.is_empty() {
        if let Some((ack_tick, player_id, end)) = acks.pop_front() {
          if ack_tick > tick - offset {
            deferred.push((ack_tick, player_id, end));
            continue;
          }

          let desync = map.ack(player_id, ack_tick).unwrap().desync;
          assert!(desync.is_none());

          if ack_tick == oldest_tick && end {
            break;
          }
        }
      }
      for v in deferred.into_iter().rev() {
        acks.push_front(v);
      }
    }
  }

  while let Some((ack_tick, player_id, _)) = acks.pop_front() {
    let desync = map.ack(player_id, ack_tick).unwrap().desync;
    assert!(desync.is_none());
  }

  assert!(map.pending_tick.is_empty());
  dbg!(&map.pending_slab.capacity());
}
