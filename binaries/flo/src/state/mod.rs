mod game;
mod net;
mod platform;

use std::sync::Arc;

use flo_config::ClientConfig;

use crate::error::Result;
use net::NetState;
pub use net::NodesConfigSenderRef;
use platform::PlatformState;
pub use platform::PlatformStateError;

#[derive(Debug)]
pub struct FloState {
  pub config: ClientConfig,
  pub platform: PlatformState,
  pub net: NetState,
}

impl FloState {
  pub async fn init() -> Result<Self> {
    let config = ClientConfig::load().unwrap_or_default();
    let (platform, net) = tokio::try_join!(PlatformState::init(&config), NetState::init(&config))?;
    Ok(FloState {
      config,
      platform,
      net,
    })
  }

  pub fn into_ref(self) -> FloStateRef {
    Arc::new(self)
  }
}

pub type FloStateRef = Arc<FloState>;
