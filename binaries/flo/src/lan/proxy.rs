use std::sync::Arc;
use tokio::sync::Notify;

use flo_event::*;
use flo_lan::{GameInfo, MdnsPublisher};

use crate::error::*;
use crate::lan::LanEvent;
use crate::lobby::LobbyGameInfo;

#[derive(Debug)]
pub struct LanProxy {
  state: Arc<State>,
}

impl LanProxy {
  pub async fn new(
    my_player_id: i32,
    game: Arc<LobbyGameInfo>,
    event_sender: EventSender<LanEvent>,
  ) -> Result<Self> {
    let game_info = GameInfo::new(
      game.game_id,
      &format!("GAME#{}-{}", game.game_id, my_player_id),
      &game.map_path,
      game.map_sha1,
      game.map_checksum,
    )?;
    Ok(Self {
      state: Arc::new(State {
        event_sender,
        game_id: game.game_id,
        dropper: Notify::new(),
        publisher: MdnsPublisher::start(game_info).await?,
      }),
    })
  }
}

impl Drop for LanProxy {
  fn drop(&mut self) {
    self.state.dropper.notify();
  }
}

#[derive(Debug)]
struct State {
  event_sender: EventSender<LanEvent>,
  game_id: i32,
  dropper: Notify,
  publisher: MdnsPublisher,
}
