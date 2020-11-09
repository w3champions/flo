mod controller;
mod error;
mod game;
mod lan;
mod node;
mod ping;
mod platform;
mod types;
mod version;
mod ws;

use crate::ws::WsListener;
use flo_state::Registry;

pub async fn start() -> Result<impl std::future::Future<Output = ()>, error::Error> {
  tracing::info!("version: {}", version::FLO_VERSION);

  let registry = Registry::new();
  let listener = registry.resolve::<WsListener>().await?;

  Ok(async move {
    let _registry = registry;
    let _listener = listener;
    futures::future::pending().await
  })
}

pub use version::FLO_VERSION;
