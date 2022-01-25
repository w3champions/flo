pub mod peer;
mod send_queue;

use crate::dispatcher::{CreateGameStreamServer, GetGameInfo};
use crate::error::Error;
use crate::error::Result;
use crate::Dispatcher;
use flo_net::{listener::FloListener, observer::ObserverConnectRejectReason, stream::FloStream};
use flo_state::Addr;
use std::time::SystemTime;
use tokio_stream::StreamExt;

pub struct StreamServer {
  listener: FloListener,
  dispatcher: Addr<Dispatcher>,
}

impl StreamServer {
  pub async fn new(dispatcher: Addr<Dispatcher>) -> Result<Self> {
    let listener = FloListener::bind_v4(flo_constants::OBSERVER_SOCKET_PORT).await?;
    Ok(Self {
      listener,
      dispatcher,
    })
  }

  pub async fn serve(mut self) -> Result<()> {
    while let Some(transport) = self.listener.incoming().try_next().await? {
      let handler = Handler {
        dispatcher: self.dispatcher.clone(),
        transport,
      };
      tokio::spawn(async move {
        if let Err(err) = handler.run().await {
          tracing::error!("stream handler: {}", err);
        }
      });
    }
    Ok(())
  }
}

struct Handler {
  dispatcher: Addr<Dispatcher>,
  transport: FloStream,
}

impl Handler {
  async fn run(mut self) -> Result<()> {
    let accepted = match self.accept().await? {
      Some(v) => v,
      None => {
        return Ok(());
      }
    };

    let server = self
      .dispatcher
      .send(CreateGameStreamServer {
        game_id: accepted.game_id,
        delay_secs: accepted.delay_secs,
      })
      .await??;

    server.run(self.transport).await?;

    Ok(())
  }

  async fn accept(&mut self) -> Result<Option<Accepted>> {
    use flo_net::observer::{PacketObserverConnect, PacketObserverConnectAccept, Version};
    let connect: PacketObserverConnect = self.transport.recv().await?;
    let token = match flo_observer::token::validate_observer_token(&connect.token) {
      Ok(v) => v,
      Err(_) => {
        self
          .reject(ObserverConnectRejectReason::InvalidToken, None)
          .await?;
        return Ok(None);
      }
    };
    let (meta, game) = match self
      .dispatcher
      .send(GetGameInfo {
        game_id: token.game_id,
      })
      .await?
    {
      Ok(game) => game,
      Err(err) => {
        match err {
          Error::GameNotFound(_) => {
            self
              .reject(ObserverConnectRejectReason::GameNotFound, None)
              .await?;
          }
          err => {
            tracing::error!(game_id = token.game_id, "get game: {}", err);
            self
              .reject(ObserverConnectRejectReason::GameNotReady, None)
              .await?;
          }
        }
        return Ok(None);
      }
    };

    let start_time = meta.started_at.timestamp();
    let now = (SystemTime::now().duration_since(SystemTime::UNIX_EPOCH))
      .unwrap()
      .as_secs() as i64;
    let expected = start_time + token.delay_secs.unwrap_or_default();

    if expected > now {
      self
        .reject(ObserverConnectRejectReason::DelayNotOver, expected.into())
        .await?;
      return Ok(None);
    }

    self
      .transport
      .send(PacketObserverConnectAccept {
        version: Some(Version {
          major: crate::version::FLO_OBSERVER_VERSION.major,
          minor: crate::version::FLO_OBSERVER_VERSION.minor,
          patch: crate::version::FLO_OBSERVER_VERSION.patch,
        }),
        game: Some(game),
        delay_secs: token.delay_secs.clone()
      })
      .await?;

    Ok(Some(Accepted {
      game_id: token.game_id,
      delay_secs: token.delay_secs,
    }))
  }

  async fn reject(
    &mut self,
    reason: ObserverConnectRejectReason,
    delay_ends_at: Option<i64>,
  ) -> Result<()> {
    use flo_net::observer::PacketObserverConnectReject;
    self
      .transport
      .send({
        let mut pkt = PacketObserverConnectReject {
          delay_ends_at,
          ..Default::default()
        };
        pkt.set_reason(reason);
        pkt
      })
      .await?;
    Ok(())
  }
}

struct Accepted {
  game_id: i32,
  delay_secs: Option<i64>,
}
