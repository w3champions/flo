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
use crate::game::db::get_all_active_game_state;
use crate::game::{GameStatus, SlotClientStatus};
use crate::node::{NodeRegistry, PlayerToken};
use crate::player::state::sender::PlayerPacketSender;

use crate::player::state::PlayerRegistry;
use crate::state::{Data, GetActorEntry};
use bs_diesel_utils::ExecutorRef;
use flo_state::*;
use start::StartGameState;
use std::collections::BTreeMap;
use std::collections::HashMap;

pub struct GameRegistry {
  db: ExecutorRef,
  player_packet_sender: PlayerPacketSender,
  nodes: Addr<NodeRegistry>,
  map: BTreeMap<i32, Container<GameActor>>,
  player_game_map: BTreeMap<i32, Vec<i32>>,
}

impl GameRegistry {
  pub async fn init(
    db: ExecutorRef,
    player_packet_sender: PlayerPacketSender,
    nodes: Addr<NodeRegistry>,
  ) -> Result<GameRegistry> {
    let games = db.exec(|conn| get_all_active_game_state(conn)).await?;
    let mut map = BTreeMap::new();
    let mut player_game_map = BTreeMap::new();

    for game in games {
      let mut players = Vec::with_capacity(game.players.len());
      let mut player_tokens = HashMap::new();

      for (id, token) in game.players {
        players.push(id);
        if let Some(token) = token.and_then(|v| PlayerToken::from_vec(id, v)) {
          player_tokens.insert(id, token.bytes);
        }
        player_game_map
          .entry(id)
          .or_insert_with(|| vec![])
          .push(game.id);
      }

      map.insert(
        game.id,
        Container::new(GameActor {
          game_id: game.id,
          db: db.clone(),
          player_packet_sender: player_packet_sender.clone(),
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
      player_packet_sender: player_packet_sender.clone(),
      nodes: nodes.clone(),
      map,
      player_game_map,
    };

    Ok(state)
  }
}

impl Actor for GameRegistry {}

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

pub struct GameActor {
  pub game_id: i32,
  pub db: ExecutorRef,
  pub player_packet_sender: PlayerPacketSender,
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
