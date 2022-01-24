mod controller;
pub mod error;
mod game;
mod lan;
mod message;
mod node;
pub mod observer;
mod ping;
mod platform;
mod version;

use crate::message::{GetPort, Listener};
use flo_state::Registry;
use observer::{ObserverClient, WatchGame};
use std::path::PathBuf;
pub use version::FLO_VERSION;

#[derive(Debug, Default, Clone)]
pub struct StartConfig {
  pub token: Option<String>,
  pub installation_path: Option<PathBuf>,
  pub user_data_path: Option<PathBuf>,
  pub controller_host: Option<String>,
  pub stats_host: Option<String>,
}

pub struct FloClient {
  _registry: Registry<StartConfig>,
  port: u16,
}

impl FloClient {
  pub fn port(&self) -> u16 {
    self.port
  }

  pub async fn start_test_game(&self) -> Result<(), error::Error> {
    use crate::platform::{Platform, StartTestGame};
    let platform = self._registry.resolve::<Platform>().await?;

    platform
      .send(StartTestGame {
        name: "TEST".to_string(),
      })
      .await??;

    Ok(())
  }

  pub async fn watch(&self, token: String) -> Result<(), error::Error> {
    let obs = self._registry.resolve::<ObserverClient>().await?;

    obs
      .send(WatchGame {
        token,
      })
      .await??;

    Ok(())
  }

  pub async fn serve(self) {
    futures::future::pending().await
  }
}

pub async fn start(config: StartConfig) -> Result<FloClient, error::Error> {
  tracing::info!("version: {}", version::FLO_VERSION);

  let registry = Registry::with_data(config);
  let listener = registry.resolve::<Listener>().await?;

  Ok(FloClient {
    port: listener.send(GetPort).await?,
    _registry: registry,
  })
}
