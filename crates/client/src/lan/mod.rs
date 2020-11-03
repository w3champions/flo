pub mod game;

use futures::lock::Mutex;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;

use flo_event::*;
use game::LanGame;

use crate::controller::LocalGameInfo;
use crate::error::*;
use crate::node::stream::NodeStreamEvent;
use crate::node::NodeInfo;
use crate::platform::PlatformStateRef;
use crate::types::{NodeGameStatus, SlotClientStatus};

#[derive(Debug)]
pub struct Lan {
  state: Arc<State>,
}

impl Lan {
  pub fn new(platform: PlatformStateRef, event_sender: Sender<LanEvent>) -> Self {
    Lan {
      state: Arc::new(State {
        platform,
        event_sender,
        active_game: Mutex::new(None),
      }),
    }
  }

  pub fn handle(&self) -> LanHandle {
    LanHandle(self.state.clone())
  }
}

#[derive(Debug)]
pub struct LanHandle(Arc<State>);

impl LanHandle {
  pub async fn replace_game(
    &self,
    my_player_id: i32,
    node: Arc<NodeInfo>,
    player_token: Vec<u8>,
    game: Arc<LocalGameInfo>,
  ) -> Result<()> {
    let game_id = game.game_id;
    let mut active_game = self.0.active_game.lock().await;
    if active_game
      .as_ref()
      .map(|g| g.is_same_game(game_id, my_player_id))
      == Some(true)
    {
      return Ok(());
    }

    let checksum = self
      .0
      .platform
      .with_storage(|storage| {
        flo_w3map::W3Map::calc_checksum(storage, &game.map_path).map_err(Into::into)
      })
      .await?;
    if checksum.sha1 == game.map_sha1 {
      let lan_game = LanGame::create(
        my_player_id,
        node,
        player_token,
        game,
        checksum,
        self.0.event_sender.clone(),
      )
      .await?;
      tracing::info!(player_id = my_player_id, game_id, "lan game created.");
      active_game.replace(lan_game);
    } else {
      active_game.take();
      return Err(Error::MapChecksumMismatch);
    }
    Ok(())
  }

  pub async fn update_game_status(
    &self,
    game_id: i32,
    status: NodeGameStatus,
    updated_player_game_client_status_map: &HashMap<i32, SlotClientStatus>,
  ) -> Result<()> {
    let mut active_game = self.0.active_game.lock().await;
    let game = if let Some(game) = active_game.as_mut() {
      game
    } else {
      return Err(Error::NotInGame);
    };

    if game.game_id() != game_id {
      tracing::error!("game id mismatch");
      return Ok(());
    }

    for (player_id, status) in updated_player_game_client_status_map {
      game.update_player_status(*player_id, *status).await?;
    }
    game.update_game_status(status).await?;

    Ok(())
  }

  pub async fn update_player_status(
    &self,
    game_id: i32,
    player_id: i32,
    status: SlotClientStatus,
  ) -> Result<()> {
    let mut active_game = self.0.active_game.lock().await;
    let game = if let Some(game) = active_game.as_mut() {
      game
    } else {
      return Err(Error::NotInGame);
    };

    if game.game_id() != game_id {
      tracing::warn!("game id mismatch");
      return Ok(());
    }

    game.update_player_status(player_id, status).await?;

    Ok(())
  }

  pub async fn stop_game(&self) {
    self.0.active_game.lock().await.take();
  }
}

#[derive(Debug)]
struct State {
  platform: PlatformStateRef,
  event_sender: Sender<LanEvent>,
  active_game: Mutex<Option<LanGame>>,
}

pub fn get_lan_game_name(game_id: i32, player_id: i32) -> String {
  use hash_ids::HashIds;
  lazy_static! {
    static ref HASHER: HashIds = HashIds::builder().with_salt("FLO").finish();
  }
  let mut value = [0_u8; 8];
  &value[0..4].copy_from_slice(&game_id.to_le_bytes());
  &value[4..8].copy_from_slice(&player_id.to_le_bytes());
  format!(
    "GAME#{}-{}",
    game_id,
    HASHER.encode(&[u64::from_le_bytes(value)])
  )
}

#[derive(Debug)]
pub enum LanEvent {
  GameDisconnected,
  NodeStreamEvent(NodeStreamEvent),
}

impl From<NodeStreamEvent> for LanEvent {
  fn from(value: NodeStreamEvent) -> Self {
    LanEvent::NodeStreamEvent(value)
  }
}

impl FloEvent for LanEvent {
  const NAME: &'static str = "LanEvent";
}
