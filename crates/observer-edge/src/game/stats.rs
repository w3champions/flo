use async_graphql::{SimpleObject};
use flo_observer::record;
use flo_w3gs::protocol::action::PlayerAction;

use super::Game;

const APM_COLLECT_INTERVAL_MS: u32 = 15 * 1000;

#[derive(Debug)]
pub struct GameStats {
  _game_id: i32,
  time: u32,
  ping: Vec<PingStats>,
  action: Vec<ActionStats>,
  apm_collect: ApmCollect,
}

impl GameStats {
  pub fn new(game: &Game) -> Self {
    Self {
      _game_id: game.id,
      time: 0,
      ping: vec![],
      action: vec![],
      apm_collect: ApmCollect::new(game),
    }
  }

  pub fn time(&self) -> u32 {
    self.time
  }

  pub fn put_rtt(&mut self, item: record::RTTStats) -> PingStats {
    let item = PingStats {
      time: item.time, 
      data: item.items.into_iter().map(|item| {
        Ping {
          player_id: item.player_id,
          min: item.min,
          max: item.max,
          avg: item.avg,
          ticks: item.ticks,
        }
      }).collect(),
    };
    self.ping.push(item.clone());
    item
  }

  pub fn put_actions(&mut self, time_increment: u16, actions: &[PlayerAction]) -> Option<ActionStats> {
    self.time += time_increment as u32;
    if let Some(item) = self.apm_collect.try_collect(self.time, actions) {
      self.action.push(item.clone());
      Some(item)
    } else {
      None
    }
  }

  pub fn make_snapshot(&self) -> GameStatsSnapshot {
    GameStatsSnapshot {
      ping: self.ping.clone(),
      action: self.action.clone(),
    }
  }
}

#[derive(Debug)]
struct ApmCollect {
  _game_id: i32,
  time: u32,
  player_slots: [Option<Action>; 24],
}

impl ApmCollect {
  fn new(game: &Game) -> Self {
    let mut player_slots: [Option<Action>; 24] = [
      None, None, None, None, None, None, None, None,
      None, None, None, None, None, None, None, None,
      None, None, None, None, None, None, None, None,
    ];
    for (idx, slot) in game.slots.iter().enumerate() {
      if let Some(ref player) = slot.player {
        if slot.settings.team != 24 {
          player_slots[idx].replace(Action {
            player_id: player.id,
            apm: 0.,
            total: 0,
          });
        }
      }
    }
    Self {
      _game_id: game.id,
      time: 0,
      player_slots,
    }
  }

  fn try_collect(&mut self, now: u32, actions: &[PlayerAction]) -> Option<ActionStats> {
    for action in actions {
      if let Some(action_id) = action.peek_action_id() {
        // Only track certain action types
        // https://github.com/PBug90/w3gjs/blob/14582c54f78f304993c23ac04e38db59d2fe960e/src/Player.ts#L326
        if [0x17, 0x18, 0x1C, 0x1D, 0x66, 0x67, 0x1E, 0x61].contains(&action_id) {
          if let Some(Some(ref mut item)) = self.player_slots.get_mut(action.player_id.saturating_sub(1) as usize) {
            item.total += 1;
          }
        }
      }
    }

    let d = now.saturating_sub(self.time);

    if d < APM_COLLECT_INTERVAL_MS {
      return None;
    }

    self.time = now;
    Some(ActionStats {
      time: now,
      data: self.player_slots.iter_mut().filter_map(|item| {
        let item = item.as_mut()?;
        let v = Action {
          apm: item.total as f32 / d as f32 * 60. * 1000.,
          ..item.clone()
        };
        item.reset();
        Some(v)
      }).collect()
    })
  }
}


#[derive(Debug, Clone, SimpleObject)]
pub struct GameStatsSnapshot {
  pub ping: Vec<PingStats>,
  pub action: Vec<ActionStats>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct PingStats 
{
  pub time: u32,
  pub data: Vec<Ping>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct Ping {
  pub player_id: i32,
  pub min: u16,
  pub max: u16,
  pub avg: f32,
  pub ticks: u16,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ActionStats 
{
  pub time: u32,
  pub data: Vec<Action>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct Action {
  pub player_id: i32,
  pub apm: f32,
  pub total: u32,
}

impl Action {
  fn reset(&mut self) {
    self.apm = 0.;
    self.total = 0;
  }
}