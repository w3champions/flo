mod proxy;

use parking_lot::RwLock;
use std::sync::Arc;

use flo_event::*;
use proxy::LanProxy;

use crate::controller::LobbyGameInfo;
use crate::error::*;

#[derive(Debug)]
pub struct Lan {
  state: Arc<State>,
}

impl Lan {
  pub fn new(event_sender: EventSender<LanEvent>) -> Self {
    Lan {
      state: Arc::new(State {
        event_sender,
        active_proxy: RwLock::new(None),
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
    self.0.active_proxy.read().is_some()
  }

  pub async fn replace_game(&self, my_player_id: i32, game: Arc<LobbyGameInfo>) -> Result<()> {
    let game_id = game.game_id;
    let proxy = LanProxy::new(my_player_id, game, self.0.event_sender.clone()).await?;
    tracing::info!(player_id = my_player_id, game_id, "lan game created.");
    *self.0.active_proxy.write() = Some(proxy);
    Ok(())
  }

  pub fn stop_game(&self) {
    self.0.active_proxy.write().take();
  }
}

#[derive(Debug)]
struct State {
  event_sender: EventSender<LanEvent>,
  active_proxy: RwLock<Option<LanProxy>>,
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
