use async_graphql::{SimpleObject};
use flo_observer::record;
use flo_w3gs::protocol::action::PlayerAction;

use super::Game;

const APM_COLLECT_INTERVAL_MS: u32 = 15 * 1000;

#[derive(Debug)]
pub struct GameStats {
  time: u32,
  ping: Vec<PingStats>,
  action: Vec<ActionStats>,
  apm_collect: ApmCollect,
}

impl GameStats {
  pub fn new(game: &Game) -> Self {
    Self {
      time: 0,
      ping: vec![],
      action: vec![],
      apm_collect: ApmCollect::new(game),
    }
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
    if time_increment == 0 {
      return None;
    }
    self.time += time_increment as u32;
    self.apm_collect.put_actions(self.time, actions)
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
      time: 0,
      player_slots,
    }
  }

  fn put_actions(&mut self, now: u32, actions: &[PlayerAction]) -> Option<ActionStats> {
    for action in actions {
      if let Some(Some(ref mut item)) = self.player_slots.get_mut(action.player_id as usize) {
        item.total += 1;
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
          apm: item.total as f32 / d as f32,
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