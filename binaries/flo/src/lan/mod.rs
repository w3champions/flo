pub mod game;

use lazy_static::lazy_static;
use parking_lot::RwLock;
use std::sync::Arc;

use flo_event::*;
use game::LanGame;

use crate::controller::LocalGameInfo;
use crate::error::*;
use crate::platform::PlatformStateRef;

#[derive(Debug)]
pub struct Lan {
  state: Arc<State>,
}

impl Lan {
  pub fn new(platform: PlatformStateRef, event_sender: EventSender<LanEvent>) -> Self {
    Lan {
      state: Arc::new(State {
        platform,
        event_sender,
        active_game: RwLock::new(None),
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
  pub fn is_active(&self) -> bool {
    self.0.active_game.read().is_some()
  }

  pub async fn replace_game(&self, my_player_id: i32, game: Arc<LocalGameInfo>) -> Result<()> {
    let game_id = game.game_id;
    let checksum = self
      .0
      .platform
      .with_storage(|storage| {
        flo_w3map::W3Map::calc_checksum(storage, &game.map_path).map_err(Into::into)
      })
      .await?;
    if checksum.sha1 == game.map_sha1 {
      let lan_game =
        LanGame::create(my_player_id, game, checksum, self.0.event_sender.clone()).await?;
      tracing::info!(player_id = my_player_id, game_id, "lan game created.");
      *self.0.active_game.write() = Some(lan_game);
    } else {
      self.0.active_game.write().take();
      return Err(Error::MapChecksumMismatch);
    }
    Ok(())
  }

  pub fn stop_game(&self) {
    self.0.active_game.write().take();
  }
}

#[derive(Debug)]
struct State {
  platform: PlatformStateRef,
  event_sender: EventSender<LanEvent>,
  active_game: RwLock<Option<LanGame>>,
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
  PlayerJoinEvent,
  PlayerChatEvent(String),
  PlayerLeftEvent,
}

impl FloEvent for LanEvent {
  const NAME: &'static str = "LanEvent";
}
