mod game;
mod lobby;
mod proxy;
pub mod slot;

pub use self::lobby::{LobbyAction, LobbyHandler};
pub use self::proxy::GameEndReason;
use crate::controller::ControllerClient;
use crate::error::*;
use crate::game::LocalGameInfo;
use crate::lan::game::proxy::PlayerEvent;
use crate::lan::game::slot::LanSlotInfo;
use crate::lan::get_lan_game_name;
use crate::node::stream::NodeConnectToken;
use crate::node::NodeInfo;
use flo_lan::{GameInfo, MdnsPublisher};
use flo_state::Addr;
use flo_task::SpawnScope;
use flo_types::node::{NodeGameStatus, SlotClientStatus};
use flo_w3gs::protocol::game::GameSettings;
use flo_w3map::MapChecksum;
use proxy::LanProxy;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::sleep;
use tracing_futures::Instrument;

pub struct LanGame {
  _scope: SpawnScope,
  state: Arc<State>,
  proxy: LanProxy,
  mdns_shutdown_notify: Arc<Notify>,
}

#[derive(Debug)]
pub struct LanGameInfo {
  pub(crate) game: Arc<LocalGameInfo>,
  pub(crate) slot_info: LanSlotInfo,
  pub(crate) map_checksum: MapChecksum,
  pub(crate) game_settings: GameSettings,
  pub(crate) lan_game_name_override: Option<String>,
}

impl LanGame {
  pub async fn create(
    game_version: String,
    my_player_id: i32,
    node: Arc<NodeInfo>,
    player_token: Vec<u8>,
    game: Arc<LocalGameInfo>,
    map_checksum: MapChecksum,
    client: Addr<ControllerClient>,
  ) -> Result<Self> {
    let mdns_shutdown_notify = Arc::new(Notify::new());

    let game_id = game.game_id;
    let game_name = get_lan_game_name(&game.name, my_player_id);
    let mut game_info = GameInfo::new(
      game.game_id,
      &game_name,
      &game.map_path.replace("\\", "/"),
      game.map_sha1,
      map_checksum.xoro,
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
        lan_game_name_override: None,
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
        let mdns_shutdown_notify = mdns_shutdown_notify.clone();
        let publisher = MdnsPublisher::start(game_version, game_info).await?;
        async move {
          let _publisher = publisher;
          tokio::select! {
            _ = scope.left() => {}
            _ = mdns_shutdown_notify.notified() => {}
          }

          sleep(Duration::from_secs(1)).await;

          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("publisher_worker")),
    );

    Ok(Self {
      _scope: scope,
      proxy,
      state,
      mdns_shutdown_notify,
    })
  }

  pub fn game_id(&self) -> i32 {
    self.state.game_id
  }

  pub async fn update_game_status(&self, status: NodeGameStatus) {
    if ![
      NodeGameStatus::Created,
      NodeGameStatus::Waiting,
      NodeGameStatus::Loading,
    ]
    .contains(&status)
    {
      self.mdns_shutdown_notify.notify_one();
    }
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

  pub fn shutdown(self) {
    tokio::spawn(async move {
      if let Err(_) =
        tokio::time::timeout(std::time::Duration::from_secs(10), self.proxy.shutdown()).await
      {
        tracing::error!("shutdown last lan game timeout.");
      }
    });
  }
}

struct State {
  game_id: i32,
  my_player_id: i32,
}
