use crate::error::{Error, Result};
use crate::types::{MapDetail, MapForceOwned, MapPlayerOwned};
use crate::StartConfig;
use flo_config::ClientConfig;
use flo_platform::error::Error as PlatformError;
use flo_platform::ClientPlatformInfo;
use flo_state::{async_trait, Actor, Context, Handler, Message, RegistryRef, Service};
use flo_w3map::MapChecksum;
use flo_w3storage::W3Storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Platform {
  installation_path: Option<PathBuf>,
  config: ClientConfig,
  info: Result<ClientPlatformInfo, PlatformStateError>,
  storage: Option<W3Storage>,
  maps: Option<Value>,
}

impl Platform {
  async fn new(installation_path: Option<PathBuf>) -> Result<Self> {
    let (config, info) = load(installation_path.clone()).await;
    Ok(Platform {
      installation_path,
      config,
      info,
      storage: None,
      maps: None,
    })
  }
}

impl Actor for Platform {}

#[async_trait]
impl Service<StartConfig> for Platform {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    Platform::new(registry.data().installation_path.clone()).await
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
    let (config, info) = load(self.installation_path.clone()).await;
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

pub struct GetClientPlatformInfo;

impl Message for GetClientPlatformInfo {
  type Result = Result<ClientPlatformInfo, PlatformStateError>;
}

#[async_trait]
impl Handler<GetClientPlatformInfo> for Platform {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetClientPlatformInfo,
  ) -> <GetClientPlatformInfo as Message>::Result {
    self.info.clone()
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

      if let Some(cfg) = self.info.as_ref().ok() {
        let s = W3Storage::new(cfg)?;
        let r = f(&s);
        self.storage = Some(s);
        r
      } else {
        Err(Error::War3NotLocated)
      }
    })
  }
}

async fn load(
  installation_path: Option<PathBuf>,
) -> (ClientConfig, Result<ClientPlatformInfo, PlatformStateError>) {
  tokio::task::block_in_place(move || {
    #[cfg(feature = "worker")]
    let config = ClientConfig {
      installation_path,
      ..Default::default()
    };
    #[cfg(not(feature = "worker"))]
    let config = ClientConfig::load()
      .map_err(|err| tracing::error!("load config: {}", err))
      .unwrap_or_default();
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
