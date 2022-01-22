use crate::error::Error;
use crate::observer::source::NetworkSource;
use crate::platform::Platform;
use crate::{ObserverGameHost, StartConfig};
use flo_state::{async_trait, Actor, Addr, RegistryRef, Service};
use tokio_util::sync::CancellationToken;

pub mod game;
mod send_queue;
pub mod source;

pub struct ObserverClient {
  platform: Addr<Platform>,
  playing: Option<Playing>,
}

impl Actor for ObserverClient {}

#[async_trait]
impl Service<StartConfig> for ObserverClient {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve::<Platform>().await?;
    Ok(ObserverClient {
      platform,
      playing: None,
    })
  }
}

struct Playing {
  ct: CancellationToken,
}

impl Drop for Playing {
  fn drop(&mut self) {
    self.ct.cancel();
  }
}
