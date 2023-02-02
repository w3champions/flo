use crate::error::{Error, Result};
use crate::observer::game::ObserverGameHost;
pub use crate::observer::game::ObserverHostShared;
use crate::observer::source::NetworkSource;
use crate::platform::{GetClientConfig, Platform};
use crate::StartConfig;
use flo_state::{async_trait, Actor, Addr, Handler, Message, RegistryRef, Service};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

pub mod game;
mod send_queue;
pub mod source;

pub struct ObserverClient {
  platform: Addr<Platform>,
  playing: Option<Playing>,
}

impl ObserverClient {
  pub fn new(platform: Addr<Platform>) -> Self {
    Self {
      platform,
      playing: None,
    }
  }
}

impl Actor for ObserverClient {}

#[async_trait]
impl Service<StartConfig> for ObserverClient {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve::<Platform>().await?;
    Ok(ObserverClient::new(platform))
  }
}

#[derive(Debug, Deserialize, Clone)]
pub struct WatchGame {
  pub token: String,
}

impl Message for WatchGame {
  type Result = Result<ObserverHostShared>;
}

#[async_trait]
impl Handler<WatchGame> for ObserverClient {
  async fn handle(
    &mut self,
    ctx: &mut flo_state::Context<Self>,
    WatchGame { token }: WatchGame,
  ) -> Result<ObserverHostShared> {
    let config = self.platform.send(GetClientConfig).await?;
    tracing::debug!("stats host: {}", config.stats_host);

    let (game, source) = NetworkSource::connect(
      &format!(
        "{}:{}",
        config.stats_host,
        flo_constants::OBSERVER_SOCKET_PORT
      ),
      token,
    )
    .await?;
    let host =
      ObserverGameHost::new(game, source.delay_secs(), source, self.platform.clone()).await?;
    let shared = host.shared();
    let ct = CancellationToken::new();
    self.playing.replace(Playing { ct: ct.clone() });
    ctx.spawn(async move {
      tokio::select! {
        _ = ct.cancelled() => {},
        r = host.play() => {
          if let Err(err) = r {
            tracing::error!("observer game host play: {}", err)
          }
        }
      }
    });
    Ok(shared)
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
