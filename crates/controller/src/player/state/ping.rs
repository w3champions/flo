use super::PlayerRegistry;

use flo_state::{async_trait, Context, Handler, Message};
use flo_types::ping::PingStats;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct UpdatePing {
  pub player_id: i32,
  pub ping_map: BTreeMap<i32, PingStats>,
}

impl Message for UpdatePing {
  type Result = ();
}

#[async_trait]
impl Handler<UpdatePing> for PlayerRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdatePing {
      player_id,
      ping_map,
    }: UpdatePing,
  ) {
    if let Some(state) = self.registry.get_mut(&player_id) {
      state.ping_map = ping_map;
    }
  }
}

pub struct GetPlayersPingSnapshot {
  pub players: Vec<i32>,
}

pub struct NodePlayersPingSnapshot {
  pub map: BTreeMap<i32, BTreeMap<i32, PingStats>>,
}

impl Message for GetPlayersPingSnapshot {
  type Result = NodePlayersPingSnapshot;
}

#[async_trait]
impl Handler<GetPlayersPingSnapshot> for PlayerRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetPlayersPingSnapshot { players }: GetPlayersPingSnapshot,
  ) -> <GetPlayersPingSnapshot as Message>::Result {
    let mut map = BTreeMap::new();
    for player_id in players {
      if let Some(stats) = self.registry.get(&player_id).map(|p| p.ping_map.clone()) {
        map.insert(player_id, stats.into_iter().collect());
      }
    }
    NodePlayersPingSnapshot { map }
  }
}
