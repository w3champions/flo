mod actor_map;

use bs_diesel_utils::{Executor, ExecutorRef};
use flo_state::{Addr, Message, Registry};

use std::sync::Arc;

use crate::error::*;
use crate::game::state::GameRegistry;

use crate::node::NodeRegistry;
use crate::player::state::PlayerRegistry;

use crate::config::ConfigStorage;
use crate::player::state::sender::PlayerPacketSender;
pub use actor_map::{ActorMapExt, GetActorEntry};

#[derive(Debug)]
pub struct Data {
  pub db: ExecutorRef,
}

pub struct ControllerState {
  pub db: ExecutorRef,
  pub registry: Registry<Data>,
  pub nodes: Addr<NodeRegistry>,
  pub games: Addr<GameRegistry>,
  pub players: Addr<PlayerRegistry>,
  pub player_packet_sender: PlayerPacketSender,
  pub config: Addr<ConfigStorage>,
}

pub type ControllerStateRef = Arc<ControllerState>;

impl ControllerState {
  pub async fn init() -> Result<Self> {
    let db = Executor::env().into_ref();

    #[cfg(not(debug_assertions))]
    {
      db.exec(|conn| crate::migration::run(conn)).await?;
    }

    let registry = Registry::with_data(Data { db: db.clone() });

    let nodes = registry.resolve().await?;
    let games = registry.resolve().await?;
    let players = registry.resolve().await?;
    let config = registry.resolve().await?;

    Ok(ControllerState {
      db,
      registry,
      nodes,
      games,
      players: players.clone(),
      player_packet_sender: PlayerPacketSender::from(players),
      config,
    })
  }

  pub async fn reload(&self) -> Result<()> {
    self.config.send(Reload).await??;
    self.nodes.send(Reload).await??;
    Ok(())
  }

  pub fn into_ref(self) -> Arc<ControllerState> {
    Arc::new(self)
  }
}

pub struct Reload;

impl Message for Reload {
  type Result = Result<()>;
}
