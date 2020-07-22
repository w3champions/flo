use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use flo_config::ClientConfig;
use flo_platform::error::Error as PlatformError;
use flo_platform::ClientPlatformInfo;
use flo_w3storage::W3Storage;

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct PlatformState {
  info: RwLock<Result<ClientPlatformInfo, PlatformStateError>>,
  storage: RwLock<Option<W3Storage>>,
  maps: RwLock<Option<Value>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PlatformStateError {
  UserDataPath,
  InstallationPath,
  Internal,
}

impl PlatformState {
  pub async fn init(config: &ClientConfig) -> Result<Self> {
    let info = tokio::task::block_in_place(move || ClientPlatformInfo::with_config(config))
      .map_err(|e| match e {
        PlatformError::NoInstallationFolder => PlatformStateError::InstallationPath,
        PlatformError::NoUserDataPath => PlatformStateError::InstallationPath,
        e => {
          tracing::error!("init platform info: {}", e);
          PlatformStateError::Internal
        }
      });
    Ok(PlatformState {
      info: RwLock::new(info),
      storage: RwLock::new(None),
      maps: RwLock::new(None),
    })
  }

  pub async fn reload(&self, config: &ClientConfig) -> Result<()> {
    let info = tokio::task::block_in_place(move || ClientPlatformInfo::with_config(config))
      .map_err(|e| match e {
        PlatformError::NoInstallationFolder => PlatformStateError::InstallationPath,
        PlatformError::NoUserDataPath => PlatformStateError::InstallationPath,
        e => {
          tracing::error!("init platform info: {}", e);
          PlatformStateError::Internal
        }
      });
    *self.info.write() = info;
    self.storage.write().take();
    self.maps.write().take();
    Ok(())
  }

  pub async fn with_storage<F, R>(&self, f: F) -> Result<R>
  where
    F: FnOnce(&W3Storage) -> Result<R> + Send,
  {
    tokio::task::block_in_place(move || {
      {
        let s = self.storage.read();
        if let Some(s) = s.as_ref() {
          return f(s);
        }
      }

      let info = self.info.read();
      if let Some(cfg) = info.as_ref().ok() {
        let s = W3Storage::new(cfg)?;
        let r = f(&s);
        *self.storage.write() = Some(s);
        r
      } else {
        Err(Error::War3NotLocated)
      }
    })
  }

  pub async fn get_map_list(&self) -> Result<Value> {
    {
      if let Some(loaded) = self.maps.read().clone() {
        return Ok(loaded);
      }
    }

    let value = self
      .with_storage(|storage| -> Result<_> {
        let paths = storage.list_storage_files("maps\\*")?;
        let paths: Vec<_> = paths
          .into_iter()
          .filter(|v| !v.contains("\\scenario\\"))
          .collect();
        let tree = flo_w3storage::path_tree::PathTree::from_paths(&paths)?;
        let value = serde_json::to_value(&tree)?;
        Ok(value)
      })
      .await?;

    *self.maps.write() = Some(value.clone());
    Ok(value)
  }

  pub fn map<F, R>(&self, f: F) -> Result<R, PlatformStateError>
  where
    F: FnOnce(&ClientPlatformInfo) -> R,
  {
    let r = self.info.read();
    match &r as &Result<_, PlatformStateError> {
      &Ok(ref info) => Ok(f(info)),
      &Err(e) => Err(e),
    }
  }
}
