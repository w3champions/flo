use crate::error::*;
use crate::game::state::player::GetGamePlayersIfNodeSelected;
use crate::game::state::{GameActor, GameRegistry};
use crate::game::GameStatus;
use flo_state::{async_trait, Container, Context, Handler, Message};
use futures::TryFutureExt;
use std::collections::btree_map::Entry;

#[derive(Debug)]
pub struct Register {
  pub id: i32,
  pub status: GameStatus,
  pub host_player: i32,
  pub players: Vec<i32>,
  pub node_id: Option<i32>,
}

impl Message for Register {
  type Result = ();
}

#[async_trait]
impl Handler<Register> for GameRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, message: Register) {
    self.register(message)
  }
}

impl GameRegistry {
  pub(crate) fn register(
    &mut self,
    Register {
      id,
      status,
      host_player,
      players,
      node_id,
    }: Register,
  ) {
    for player in &players {
      self.add_game_player(id, *player);
    }
    self.map.insert(
      id,
      Container::new(GameActor {
        game_id: id,
        db: self.db.clone(),
        player_packet_sender: self.player_packet_sender.clone(),
        nodes: self.nodes.clone(),
        status,
        host_player,
        players,
        selected_node_id: node_id,
        start_state: None,
        player_tokens: Default::default(),
        player_client_status_map: Default::default(),
      }),
    );
  }
}

#[derive(Debug)]
pub struct Remove {
  pub game_id: i32,
}

impl Message for Remove {
  type Result = ();
}

#[async_trait]
impl Handler<Remove> for GameRegistry {
  async fn handle(&mut self, ctx: &mut Context<Self>, Remove { game_id: id }: Remove) {
    if let Some(container) = self.map.remove(&id) {
      let addr = ctx.addr();
      ctx.spawn(async move {
        match tokio::time::timeout(std::time::Duration::from_secs(3), container.shutdown()).await {
          Ok(Ok(state)) => {
            let players = state.players;
            for player_id in players {
              addr
                .notify(RemoveGamePlayer {
                  game_id: id,
                  player_id,
                })
                .await
                .ok();
            }
            tracing::debug!(game_id = id, "game shutdown completed");
          }
          Ok(Err(err)) => {
            tracing::warn!(game_id = id, "Remove: fetch state: {}", err);
          }
          Err(_) => {
            tracing::warn!(game_id = id, "Remove: shutdown game actor timeout");
          }
        }
      })
    }
  }
}

pub struct AddGamePlayer {
  pub game_id: i32,
  pub player_id: i32,
}

impl Message for AddGamePlayer {
  type Result = ();
}

#[async_trait]
impl Handler<AddGamePlayer> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    AddGamePlayer { game_id, player_id }: AddGamePlayer,
  ) {
    self.add_game_player(game_id, player_id)
  }
}

pub struct RemoveGamePlayer {
  pub game_id: i32,
  pub player_id: i32,
}

impl Message for RemoveGamePlayer {
  type Result = ();
}

#[async_trait]
impl Handler<RemoveGamePlayer> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    RemoveGamePlayer { game_id, player_id }: RemoveGamePlayer,
  ) {
    self.remove_game_player(game_id, player_id)
  }
}

pub struct ResolveGamePlayerPingBroadcastTargets {
  pub player_id: i32,
  pub node_ids: Vec<i32>,
}

impl Message for ResolveGamePlayerPingBroadcastTargets {
  type Result = Result<Vec<i32>>;
}

#[async_trait]
impl Handler<ResolveGamePlayerPingBroadcastTargets> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    ResolveGamePlayerPingBroadcastTargets {
      player_id,
      node_ids,
    }: ResolveGamePlayerPingBroadcastTargets,
  ) -> Result<Vec<i32>> {
    use futures::stream::FuturesUnordered;
    use futures::StreamExt;
    let games = match self.player_game_map.get(&player_id) {
      Some(v) => v,
      None => return Ok(vec![]),
    };
    let task: FuturesUnordered<_> = games
      .iter()
      .filter_map(|game_id| {
        self.map.get(game_id).map(|v| {
          v.send(GetGamePlayersIfNodeSelected {
            node_ids: node_ids.clone(),
            include_player: player_id,
          })
          .map_err(Error::from)
        })
      })
      .collect();
    let mut player_ids: Vec<i32> = task
      .collect::<Vec<_>>()
      .await
      .into_iter()
      .collect::<Result<Vec<_>>>()?
      .into_iter()
      .filter_map(std::convert::identity)
      .flatten()
      .collect();
    player_ids.sort();
    player_ids.dedup();
    player_ids.retain(|v| *v != player_id);
    Ok(player_ids)
  }
}

impl GameRegistry {
  fn add_game_player(&mut self, game_id: i32, player_id: i32) {
    self
      .player_game_map
      .entry(player_id)
      .or_insert_with(|| vec![])
      .push(game_id);
  }

  fn remove_game_player(&mut self, game_id: i32, player_id: i32) {
    match self.player_game_map.entry(player_id) {
      Entry::Vacant(_entry) => {}
      Entry::Occupied(mut entry) => {
        entry.get_mut().retain(|v| *v != game_id);
        if entry.get().is_empty() {
          entry.remove();
        }
      }
    }
  }
}
