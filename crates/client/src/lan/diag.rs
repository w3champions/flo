use crate::error::Result;
use crate::game::LocalGameInfo;
use crate::lan::game::{LanGameInfo, LobbyAction, LobbyHandler};
use flo_lan::MdnsPublisher;
use flo_types::game::{
  GameInfo, GameStatus, Map, PlayerInfo, PlayerSource, Slot, SlotSettings, SlotStatus,
};
use flo_types::node::SlotClientStatus;
use flo_util::binary::CString;
use flo_w3gs::constants::GameSettingFlags;
use flo_w3gs::game::GameSettings;
use flo_w3gs::net::W3GSListener;
use flo_w3map::MapChecksum;
use futures::TryStreamExt;
use std::sync::Arc;
use tokio::sync::watch::channel;

pub async fn run_test_lobby(
  name: &str,
  map_path: &str,
  map_width: u16,
  map_height: u16,
  map_checksum: MapChecksum,
) -> Result<Option<LobbyAction>> {
  let map_sha1 = map_checksum.sha1;

  let player = PlayerInfo {
    id: 1,
    name: "Player 1".to_string(),
    source: PlayerSource::Test,
  };
  let game = GameInfo {
    id: 0,
    name: name.to_string(),
    status: GameStatus::Preparing,
    map: Map {
      sha1: map_checksum.sha1.to_vec(),
      checksum: 0xFFFFFFFF,
      path: map_path.to_string(),
    },
    slots: vec![Slot {
      player: Some(player.clone()),
      settings: SlotSettings {
        status: SlotStatus::Occupied,
        handicap: 100,
        ..Default::default()
      },
      client_status: SlotClientStatus::Pending,
    }],
    node: None,
    is_private: false,
    is_live: false,
    random_seed: 0,
    created_by: None,
  };

  let info = LanGameInfo {
    game: Arc::new(LocalGameInfo::from_game_info(1, &game)?),
    slot_info: crate::lan::game::slot::build_player_slot_info(1, game.random_seed, &game.slots)?,
    map_checksum,
    game_settings: GameSettings {
      game_setting_flags: GameSettingFlags::SPEED_FAST
        | GameSettingFlags::TERRAIN_DEFAULT
        | GameSettingFlags::OBS_ENABLED
        | GameSettingFlags::TEAMS_TOGETHER
        | GameSettingFlags::TEAMS_FIXED,
      unk_1: 0,
      map_width,
      map_height,
      map_checksum: 0xFFFFFFFF,
      map_path: CString::new(map_path).map_err(|_| flo_lan::error::Error::NullByteInString)?,
      host_name: CString::new("FLO").unwrap(),
      map_sha1,
    },
  };

  let (_tx, mut rx) = channel(None);

  let mut listener = W3GSListener::bind().await.unwrap();
  tracing::debug!("listening on {}", listener.local_addr());
  let port = listener.port();

  let lan_game_info = {
    let mut game_info = flo_lan::GameInfo::new(1, name, map_path, map_sha1, 0xFFFFFFFF)?;
    game_info.set_port(port);
    game_info
  };

  let _p = MdnsPublisher::start(lan_game_info).await?;

  while let Some(mut stream) = listener.incoming().try_next().await? {
    return LobbyHandler::new(&info, &mut stream, None, &mut rx)
      .run()
      .await
      .map(Some);
  }

  Ok(None)
}
