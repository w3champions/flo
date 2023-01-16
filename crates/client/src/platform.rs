use crate::error::{Error, Result};
use crate::StartConfig;
use flo_config::ClientConfig;
use flo_platform::error::Error as PlatformError;
use flo_platform::ClientPlatformInfo;
use flo_state::{async_trait, Actor, Context, Handler, Message, RegistryRef, Service};
use flo_types::game::{MapDetail, MapForceOwned, MapPlayerOwned};
use flo_w3map::{MapChecksum, W3Map};
use flo_w3storage::W3Storage;
use futures::future::{abortable, AbortHandle};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug)]
pub struct Platform {
  start_config: StartConfig,
  config: ClientConfig,
  info: Result<ClientPlatformInfo, PlatformStateError>,
  storage: Option<W3Storage>,
  maps: Option<Value>,
  test_game_abort_handle: Option<AbortHandle>,
}

impl Platform {
  pub async fn new(start_config: &StartConfig) -> Result<Self> {
    let (config, info) = load(start_config).await;
    Ok(Platform {
      start_config: start_config.clone(),
      config,
      info,
      storage: None,
      maps: None,
      test_game_abort_handle: None,
    })
  }
}

impl Actor for Platform {}

#[async_trait]
impl Service<StartConfig> for Platform {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    Platform::new(registry.data()).await
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PlatformStateError {
  UserDataPath,
  InstallationPath,
  Internal,
}

pub struct Reload;

impl Message for Reload {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<Reload> for Platform {
  async fn handle(&mut self, _: &mut Context<Self>, _: Reload) -> <Reload as Message>::Result {
    let (config, info) = load(&self.start_config).await;
    self.config = config;
    self.info = info;
    self.maps.take();
    Ok(())
  }
}

pub struct GetMapList;

impl Message for GetMapList {
  type Result = Result<Value>;
}

#[async_trait]
impl Handler<GetMapList> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetMapList,
  ) -> <GetMapList as Message>::Result {
    {
      if let Some(loaded) = self.maps.clone() {
        return Ok(loaded);
      }
    }

    let paths = self
      .with_storage(move |storage| storage.list_storage_files("maps\\*").map_err(Into::into))
      .await?;
    let paths: Vec<_> = paths
      .into_iter()
      .filter(|v| !v.contains("\\scenario\\"))
      .collect();
    let tree = flo_w3storage::path_tree::PathTree::from_paths(&paths)?;
    let value = serde_json::to_value(&tree)?;
    self.maps = Some(value.clone());
    Ok(value)
  }
}

#[derive(Default)]
pub struct GetClientPlatformInfo {
  pub force_reload: bool,
}

impl Message for GetClientPlatformInfo {
  type Result = Result<ClientPlatformInfo, PlatformStateError>;
}

#[async_trait]
impl Handler<GetClientPlatformInfo> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetClientPlatformInfo { force_reload }: GetClientPlatformInfo,
  ) -> <GetClientPlatformInfo as Message>::Result {
    if force_reload {
      let (config, info) = load(&self.start_config).await;
      self.config = config;
      self.info = info;
      self.info.clone()
    } else {
      self.info.clone()
    }
  }
}

pub struct CalcMapChecksum {
  pub path: String,
}

impl Message for CalcMapChecksum {
  type Result = Result<MapChecksum>;
}

#[async_trait]
impl Handler<CalcMapChecksum> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CalcMapChecksum { path }: CalcMapChecksum,
  ) -> <CalcMapChecksum as Message>::Result {
    self
      .with_storage(|storage| flo_w3map::W3Map::calc_checksum(storage, &path).map_err(Into::into))
      .await
  }
}

pub struct OpenMap {
  pub path: String,
}

pub struct OpenMapReply {
  pub map: W3Map,
  pub checksum: MapChecksum,
}

impl Message for OpenMap {
  type Result = Result<OpenMapReply>;
}

#[async_trait]
impl Handler<OpenMap> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    OpenMap { path }: OpenMap,
  ) -> <OpenMap as Message>::Result {
    let (map, checksum) = self
      .with_storage(|storage| {
        flo_w3map::W3Map::open_storage_with_checksum(storage, &path).map_err(Into::into)
      })
      .await?;
    Ok(OpenMapReply { map, checksum })
  }
}

pub struct GetClientConfig;

impl Message for GetClientConfig {
  type Result = ClientConfig;
}

#[async_trait]
impl Handler<GetClientConfig> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetClientConfig,
  ) -> <GetClientConfig as Message>::Result {
    self.config.clone()
  }
}

pub struct GetMapDetail {
  pub path: String,
}

impl Message for GetMapDetail {
  type Result = Result<MapDetail>;
}

#[async_trait]
impl Handler<GetMapDetail> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetMapDetail { path }: GetMapDetail,
  ) -> <GetMapDetail as Message>::Result {
    use flo_w3map::W3Map;
    self
      .with_storage(move |storage| {
        let (map, checksum) = W3Map::open_storage_with_checksum(storage, &path)?;
        let (width, height) = map.dimension();
        Ok(MapDetail {
          path,
          sha1: checksum.get_sha1_hex_string(),
          crc32: checksum.crc32,
          name: map.name().to_string(),
          author: map.author().to_string(),
          description: map.description().to_string(),
          width,
          height,
          preview_jpeg_base64: base64::encode(map.render_preview_jpeg()),
          suggested_players: map.suggested_players().to_string(),
          num_players: map.num_players(),
          players: map
            .get_players()
            .into_iter()
            .map(|p| MapPlayerOwned {
              name: p.name.to_string(),
              r#type: p.r#type,
              race: p.race,
              flags: p.flags,
            })
            .collect(),
          forces: map
            .get_forces()
            .into_iter()
            .map(|f| MapForceOwned {
              name: f.name.to_string(),
              flags: f.flags,
              player_set: f.player_set,
            })
            .collect(),
        })
      })
      .await
  }
}

#[derive(Debug, Deserialize)]
pub struct StartTestGame {
  pub name: String,
}

impl Message for StartTestGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartTestGame> for Platform {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    StartTestGame { name }: StartTestGame,
  ) -> <StartTestGame as Message>::Result {
    let next = self.start_test_game(ctx, name).await?;
    if let Some(handle) = self.test_game_abort_handle.replace(next) {
      handle.abort()
    }
    Ok(())
  }
}

pub struct KillTestGame;

impl Message for KillTestGame {
  type Result = ();
}

#[async_trait]
impl Handler<KillTestGame> for Platform {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    _: KillTestGame,
  ) -> <KillTestGame as Message>::Result {
    self
      .test_game_abort_handle
      .take()
      .map(|handle| handle.abort());
  }
}

impl Platform {
  pub async fn with_storage<F, R>(&mut self, f: F) -> Result<R>
  where
    F: FnOnce(&W3Storage) -> Result<R> + Send,
  {
    tokio::task::block_in_place(move || {
      {
        if let Some(s) = self.storage.as_ref() {
          return f(s);
        }
      }

      match self.info.as_ref() {
        Ok(cfg) => {
          let s = W3Storage::new(cfg)?;
          let r = f(&s);
          self.storage = Some(s);
          r
        }
        Err(e) => {
          tracing::debug!("Failed to locate WC3: {:?}", e);
          Err(Error::War3NotLocated)
        }
      }
    })
  }

  async fn start_test_game(
    &mut self,
    ctx: &mut Context<Self>,
    name: String,
  ) -> Result<AbortHandle> {
    tracing::debug!("starting test game: {}", name);

    const MAP_PATH: &str = r#"maps\(2)bootybay.w3m"#;
    let (map, checksum) = self
      .with_storage(|storage| {
        W3Map::open_storage_with_checksum(storage, MAP_PATH).map_err(Error::from)
      })
      .await?;
    let info = self.info.clone();
    let (f, handle) = abortable(async move {
      let (width, height) = map.dimension();
      let res = {
        let game_version = info.map(|info| info.version.clone());
        match game_version {
          Ok(game_version) => {
            crate::lan::diag::run_test_lobby(
              game_version,
              &name,
              MAP_PATH,
              width as u16,
              height as u16,
              checksum,
            )
            .await
          }
          Err(_) => Err(Error::War3NotLocated),
        }
      };
      match res {
        Ok(res) => tracing::debug!("test game ended: {:?}", res),
        Err(err) => {
          tracing::error!("start test game: {}", err);
        }
      }
    });
    ctx.spawn(f.map(|_| ()));
    Ok(handle)
  }
}

async fn load(
  start_config: &StartConfig,
) -> (ClientConfig, Result<ClientPlatformInfo, PlatformStateError>) {
  tokio::task::block_in_place(move || {
    #[cfg(feature = "worker")]
    let config = ClientConfig {
      installation_path: start_config.installation_path.clone(),
      user_data_path: start_config.user_data_path.clone(),
      version: start_config.version.clone(),
      controller_host: start_config
        .controller_host
        .clone()
        .unwrap_or_else(|| flo_constants::CONTROLLER_HOST.to_string()),
      stats_host: start_config
        .stats_host
        .clone()
        .unwrap_or_else(|| flo_constants::STATS_HOST.to_string()),
      ptr: start_config.ptr,
      ..Default::default()
    };

    #[cfg(not(feature = "worker"))]
    let config = {
      let _ = start_config;
      ClientConfig::load().unwrap_or_default()
    };
    let info = ClientPlatformInfo::with_config(&config).map_err(|e| match e {
      PlatformError::NoInstallationFolder => PlatformStateError::InstallationPath,
      PlatformError::NoUserDataPath => PlatformStateError::InstallationPath,
      e => {
        tracing::error!("init platform info: {}", e);
        PlatformStateError::Internal
      }
    });
    (config, info)
  })
}
