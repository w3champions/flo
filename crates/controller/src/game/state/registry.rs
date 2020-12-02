use crate::error::*;
use crate::game::state::{GameActor, GameRegistry};
use crate::game::GameStatus;
use flo_state::{async_trait, Container, Context, Handler, Message};
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
      self.game_players_map.remove(&id);
      self.game_node_map.remove(&id);

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

pub struct UpdateGameNodeCache {
  pub game_id: i32,
  pub node_id: Option<i32>,
}

impl Message for UpdateGameNodeCache {
  type Result = ();
}

#[async_trait]
impl Handler<UpdateGameNodeCache> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateGameNodeCache { game_id, node_id }: UpdateGameNodeCache,
  ) {
    if let Some(node_id) = node_id {
      self.game_node_map.insert(game_id, node_id);
    } else {
      self.game_node_map.remove(&game_id);
    }
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
    let games = match self.player_games_map.get(&player_id) {
      Some(v) => v,
      None => return Ok(vec![]),
    };

    let mut player_ids: Vec<i32> = games
      .into_iter()
      .filter_map(|game_id| {
        let node_id = self.game_node_map.get(game_id)?;
        if node_ids.contains(node_id) {
          self.game_players_map.get(game_id).cloned()
        } else {
          None
        }
      })
      .flatten()
      .collect::<Vec<_>>();
    player_ids.sort();
    player_ids.dedup();
    player_ids.retain(|v| *v != player_id);

    Ok(player_ids)
  }
}

impl GameRegistry {
  fn add_game_player(&mut self, game_id: i32, player_id: i32) {
    self
      .player_games_map
      .entry(player_id)
      .or_insert_with(|| vec![])
      .push(game_id);
    self
      .game_players_map
      .entry(game_id)
      .or_insert_with(|| vec![])
      .push(player_id);
  }

  fn remove_game_player(&mut self, game_id: i32, player_id: i32) {
    match self.player_games_map.entry(player_id) {
      Entry::Vacant(_entry) => {}
      Entry::Occupied(mut entry) => {
        entry.get_mut().retain(|v| *v != game_id);
        if entry.get().is_empty() {
          entry.remove();
        }
      }
    }

    match self.game_players_map.entry(game_id) {
      Entry::Vacant(_entry) => {}
      Entry::Occupied(mut entry) => {
        entry.get_mut().retain(|v| *v != player_id);
        if entry.get().is_empty() {
          entry.remove();
        }
      }
    }
  }
}
