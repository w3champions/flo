pub mod cancel;
pub mod create;
pub mod join;
pub mod leave;
pub mod node;
pub mod player;
pub mod registry;
pub mod slot;
pub mod start;
pub mod status;

pub use status::{GameSlotClientStatusUpdate, GameStatusUpdate};

use crate::error::*;
use crate::game::db::{get_all_active_game_state, get_expired_games};
use crate::game::{GameStatus, SlotClientStatus};
use crate::node::{NodeRegistry, PlayerToken};
use crate::player::state::sender::PlayerRegistryHandle;

use crate::game::state::cancel::CancelGame;
use crate::game::state::registry::Remove;
use crate::player::state::PlayerRegistry;
use crate::state::{Data, GetActorEntry};
use bs_diesel_utils::ExecutorRef;
use flo_state::*;
use start::StartGameState;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

const GAME_INACTIVE_CHECK_INTERVAL: Duration = Duration::from_secs(3600 * 30);

pub struct GameRegistry {
  db: ExecutorRef,
  players: PlayerRegistryHandle,
  nodes: Addr<NodeRegistry>,
  map: BTreeMap<i32, Container<GameActor>>,
  player_games_map: BTreeMap<i32, Vec<i32>>,
  game_players_map: BTreeMap<i32, Vec<i32>>,
  game_node_map: BTreeMap<i32, i32>,
}

impl GameRegistry {
  pub async fn init(
    db: ExecutorRef,
    player_packet_sender: PlayerRegistryHandle,
    nodes: Addr<NodeRegistry>,
  ) -> Result<GameRegistry> {
    let games = db.exec(|conn| get_all_active_game_state(conn)).await?;
    let mut map = BTreeMap::new();
    let mut player_games_map = BTreeMap::new();
    let mut game_players_map = BTreeMap::new();
    let mut game_node_map = BTreeMap::new();

    for game in games {
      let mut players = Vec::with_capacity(game.players.len());
      let mut player_tokens = HashMap::new();

      game_players_map.insert(game.id, game.players.iter().map(|t| t.0).collect());
      for (id, token) in game.players {
        players.push(id);
        if let Some(token) = token.and_then(|v| PlayerToken::from_vec(id, v)) {
          player_tokens.insert(id, token.bytes);
        }
        player_games_map
          .entry(id)
          .or_insert_with(|| vec![])
          .push(game.id);
      }

      if let Some(node_id) = game.node_id.clone() {
        game_node_map.insert(game.id, node_id);
      }

      map.insert(
        game.id,
        Container::new(GameActor {
          game_id: game.id,
          db: db.clone(),
          player_reg: player_packet_sender.clone(),
          nodes: nodes.clone(),
          status: game.status,
          host_player: game.created_by,
          players,
          selected_node_id: game.node_id,
          start_state: None,
          player_tokens,
          player_client_status_map: Default::default(),
        }),
      );
    }

    let state = GameRegistry {
      db: db.clone(),
      players: player_packet_sender.clone(),
      nodes: nodes.clone(),
      map,
      player_games_map,
      game_players_map,
      game_node_map,
    };

    Ok(state)
  }

  async fn remove_expired_games(&mut self, ctx: &mut Context<Self>) -> Result<()> {
    let ids = self.db.exec(|conn| get_expired_games(conn)).await?;

    let mut cancelled = vec![];
    for id in ids {
      if let Some(c) = self.map.get_mut(&id) {
        if let Err(err) = c.send(CancelGame { player_id: None }).await {
          tracing::error!(game_id = id, "cancel expired game: {}", err);
        } else {
          cancelled.push(id)
        }
      }
    }

    if !cancelled.is_empty() {
      let addr = ctx.addr();
      ctx.spawn(async move {
        for game_id in cancelled {
          if let Err(err) = addr.send(Remove { game_id }).await {
            tracing::error!(game_id, "remove cancelled game: {}", err);
          }
        }
      })
    }

    Ok(())
  }
}

#[async_trait]
impl Actor for GameRegistry {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    self.handle(ctx, RemoveExpiredGames).await;
  }
}

#[async_trait]
impl Service<Data> for GameRegistry {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<Data>) -> Result<Self, Self::Error> {
    let players = registry.resolve::<PlayerRegistry>().await?;
    let nodes = registry.resolve::<NodeRegistry>().await?;
    Self::init(registry.data().db.clone(), players.into(), nodes).await
  }
}

#[async_trait]
impl Handler<GetActorEntry<GameActor>> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    message: GetActorEntry<GameActor>,
  ) -> <GetActorEntry<GameActor, i32> as Message>::Result {
    self.map.get(message.key()).map(|v| v.addr())
  }
}

struct RemoveExpiredGames;

impl Message for RemoveExpiredGames {
  type Result = ();
}

#[async_trait]
impl Handler<RemoveExpiredGames> for GameRegistry {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    _: RemoveExpiredGames,
  ) -> <RemoveExpiredGames as Message>::Result {
    if let Err(err) = self.remove_expired_games(ctx).await {
      tracing::error!("remove expired games: {}", err);
    }
    let addr = ctx.addr();
    ctx.spawn(async move {
      sleep(GAME_INACTIVE_CHECK_INTERVAL).await;
      addr.notify(RemoveExpiredGames).await.ok();
    });
  }
}

pub struct GameActor {
  pub game_id: i32,
  pub db: ExecutorRef,
  pub player_reg: PlayerRegistryHandle,
  pub nodes: Addr<NodeRegistry>,
  pub status: GameStatus,
  pub host_player: i32,
  pub players: Vec<i32>,
  pub selected_node_id: Option<i32>,
  pub start_state: Option<Container<StartGameState>>,
  pub player_tokens: HashMap<i32, [u8; 16]>,
  pub player_client_status_map: HashMap<i32, SlotClientStatus>,
}

impl Actor for GameActor {}

impl GameActor {
  fn started(&self) -> bool {
    self.start_state.is_some() || !self.player_tokens.is_empty()
  }
}
