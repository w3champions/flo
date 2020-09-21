use super::PlayerRegistry;


use flo_state::{async_trait, Context, Handler, Message};
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct UpdatePing {
  pub player_id: i32,
  pub items: Vec<PingUpdate>,
}

impl Message for UpdatePing {
  type Result = ();
}

#[derive(Debug, Clone)]
pub struct PingUpdate {
  pub node_id: i32,
  pub avg: Option<u32>,
  pub current: Option<u32>,
  pub loss_rate: f32,
}

#[async_trait]
impl Handler<UpdatePing> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, UpdatePing { player_id, items }: UpdatePing) {
    if let Some(state) = self.registry.get_mut(&player_id) {
      for update in items {
        match state.ping_map.entry(update.node_id) {
          Entry::Vacant(entry) => {
            entry.insert(PingStats {
              min: update.current.clone(),
              max: update.current.clone(),
              avg: update.avg,
              current: update.current,
              loss_rate: update.loss_rate,
            });
          }
          Entry::Occupied(entry) => {
            let entry = entry.into_mut();
            if let Some(current) = update.current.clone() {
              entry.min = entry
                .min
                .map(|v| std::cmp::min(v, current))
                .or(Some(current));
              entry.max = entry
                .max
                .map(|v| std::cmp::max(v, current))
                .or(Some(current));
            }
            entry.current = entry.current;
            entry.avg = update.avg;
            entry.loss_rate = update.loss_rate;
          }
        }
      }
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

#[derive(Debug, Clone, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PingStats))]
pub struct PingStats {
  pub min: Option<u32>,
  pub max: Option<u32>,
  pub avg: Option<u32>,
  pub current: Option<u32>,
  pub loss_rate: f32,
}
