use crate::error::{Result, Error};
use crate::observer::game::ObserverGameHost;
use crate::observer::source::NetworkSource;
use crate::platform::{Platform, GetClientConfig};
use crate::StartConfig;
use flo_state::{async_trait, Actor, Addr, RegistryRef, Service, Message, Handler};
use serde::Deserialize;
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

#[derive(Debug, Deserialize)]
pub struct WatchGame {
  pub token: String,
}

impl Message for WatchGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<WatchGame> for ObserverClient {
  async fn handle(&mut self, ctx: &mut flo_state::Context<Self>, WatchGame { token }: WatchGame) -> Result<()> {
    let config = self.platform.send(GetClientConfig).await?;
    tracing::debug!("stats host: {}", config.stats_host);

    let (game, source) = NetworkSource::connect(&format!("{}:{}", config.stats_host, flo_constants::OBSERVER_SOCKET_PORT), token).await?;
    let host = ObserverGameHost::new(game, source, self.platform.clone()).await?;
    let ct = CancellationToken::new();
    self.playing.replace(Playing {
      ct: ct.clone(),
    });
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
    Ok(())
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
