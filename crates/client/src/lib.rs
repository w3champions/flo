mod controller;
mod error;
mod game;
mod lan;
mod message;
mod node;
mod ping;
mod platform;
mod types;
mod version;

use crate::message::{GetPort, Listener};
use flo_state::Registry;
use std::path::PathBuf;
pub use version::FLO_VERSION;

#[derive(Default)]
pub struct StartConfig {
  pub token: Option<String>,
  pub installation_path: Option<PathBuf>,
}

pub struct FloClient {
  _registry: Registry<StartConfig>,
  port: u16,
}

impl FloClient {
  pub fn port(&self) -> u16 {
    self.port
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
