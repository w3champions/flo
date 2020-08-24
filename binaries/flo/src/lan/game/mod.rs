mod listener;
pub mod slot;

use std::sync::Arc;
use tokio::sync::Notify;

use flo_event::*;
use flo_lan::{GameInfo, MdnsPublisher};
use flo_w3gs::protocol::game::GameSettings;
use flo_w3map::MapChecksum;

use crate::controller::LocalGameInfo;
use crate::error::*;
use crate::lan::game::slot::LanSlotInfo;
use crate::lan::{get_lan_game_name, LanEvent};

use listener::LanGameListener;

#[derive(Debug)]
pub struct LanGame {
  state: Arc<State>,
  listener: LanGameListener,
}

#[derive(Debug)]
pub struct LanGameInfo {
  pub(crate) game: Arc<LocalGameInfo>,
  pub(crate) slot_info: LanSlotInfo,
  pub(crate) map_checksum: MapChecksum,
  pub(crate) game_settings: GameSettings,
}

impl LanGame {
  pub async fn create(
    my_player_id: i32,
    game: Arc<LocalGameInfo>,
    map_checksum: MapChecksum,
    event_sender: EventSender<LanEvent>,
  ) -> Result<Self> {
    let game_id = game.game_id;
    let mut game_info = GameInfo::new(
      game.game_id,
      &get_lan_game_name(game.game_id, my_player_id),
      &game.map_path.replace("\\", "/"),
      game.map_sha1,
      game.map_checksum,
    )?;
    let listener = LanGameListener::bind(
      LanGameInfo {
        slot_info: crate::lan::game::slot::build_player_slot_info(
          my_player_id,
          game.random_seed,
          &game.slots,
        )?,
        game,
        map_checksum,
        game_settings: game_info.data.settings.clone(),
      },
      event_sender.clone(),
    )
    .await?;
    game_info.set_port(listener.port());
    Ok(Self {
      listener,
      state: Arc::new(State {
        event_sender,
        game_id,
        dropper: Notify::new(),
        publisher: MdnsPublisher::start(game_info).await?,
      }),
    })
  }
}

impl Drop for LanGame {
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
