mod game;
mod lobby;
mod proxy;
pub mod slot;

use std::sync::Arc;
use std::time::Duration;
use tokio::time::delay_for;
use tracing_futures::Instrument;

use flo_lan::{GameInfo, MdnsPublisher};
use flo_task::SpawnScope;
use flo_w3gs::protocol::game::GameSettings;
use flo_w3map::MapChecksum;

use crate::error::*;
use crate::game::LocalGameInfo;
use crate::lan::game::slot::LanSlotInfo;
#[cfg(not(feature = "worker"))]
use crate::lan::get_lan_game_name;
use crate::node::stream::NodeConnectToken;
use crate::node::NodeInfo;

use crate::controller::ControllerClient;
use crate::lan::game::proxy::PlayerEvent;
use crate::types::{NodeGameStatus, SlotClientStatus};
use flo_state::Addr;
use proxy::LanProxy;

pub struct LanGame {
  _scope: SpawnScope,
  state: Arc<State>,
  proxy: LanProxy,
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
    node: Arc<NodeInfo>,
    player_token: Vec<u8>,
    game: Arc<LocalGameInfo>,
    map_checksum: MapChecksum,
    client: Addr<ControllerClient>,
  ) -> Result<Self> {
    let game_id = game.game_id;
    #[cfg(not(feature = "worker"))]
    let game_name = get_lan_game_name(game.game_id, my_player_id);
    #[cfg(feature = "worker")]
    let game_name = format!("{}-{}", game.name, my_player_id);
    let mut game_info = GameInfo::new(
      game.game_id,
      &game_name,
      &game.map_path.replace("\\", "/"),
      game.map_sha1,
      game.map_checksum,
    )?;
    let token = NodeConnectToken::from_vec(player_token).ok_or_else(|| Error::InvalidNodeToken)?;
    let proxy = LanProxy::start(
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
      node,
      token,
      client.clone(),
    )
    .await?;
    game_info.set_port(proxy.port());
    let scope = SpawnScope::new();
    let state = Arc::new(State {
      game_id,
      my_player_id,
    });
    tokio::spawn(
      {
        let mut scope = scope.handle();
        let mut publisher = MdnsPublisher::start(game_info).await?;
        async move {
          loop {
            tokio::select! {
              _ = scope.left() => {
                break;
              }
              _ = delay_for(Duration::from_secs(5)) => {
                if let Err(err) = publisher.refresh().await {
                  tracing::error!("mdns refresh: {}", err);
                  break;
                }
              }
            }
          }
          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("publisher_worker")),
    );

    Ok(Self {
      _scope: scope,
      proxy,
      state,
    })
  }

  pub fn game_id(&self) -> i32 {
    self.state.game_id
  }

  pub async fn update_game_status(&self, status: NodeGameStatus) {
    self.proxy.dispatch_game_status_change(status).await;
  }

  pub async fn update_player_status(&mut self, player_id: i32, status: SlotClientStatus) {
    self
      .proxy
      .dispatch_player_event(PlayerEvent::PlayerStatusChange { player_id, status })
      .await;
  }

  pub fn is_same_game(&self, game_id: i32, my_player_id: i32) -> bool {
    self.state.game_id == game_id && self.state.my_player_id == my_player_id
  }
}

struct State {
  game_id: i32,
  my_player_id: i32,
}
