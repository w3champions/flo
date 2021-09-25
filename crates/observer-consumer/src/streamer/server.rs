use std::time::SystemTime;

use crate::{archiver::ArchiverHandle, error::Result, token::ObserverToken};
use flo_grpc::controller::flo_controller_client::FloControllerClient;
use flo_grpc::Channel;
use flo_net::{listener::FloListener, observer::ObserverConnectRejectReason, stream::FloStream};
use flo_state::{Actor, Addr, Owner};
use futures::stream::TryStreamExt;
use tempfile::{tempdir, TempDir};

const MAX_IN_MEM_GAME: usize = 300;
type MemCacheMgr = crate::mem_cache::MemCacheMgr<MAX_IN_MEM_GAME>;

pub struct StreamServer {
  swap_dir: TempDir,
  ctrl: FloControllerClient<Channel>,
  archiver: ArchiverHandle,
  mem_cache: Owner<MemCacheMgr>,
  listener: FloListener,
}

impl StreamServer {
  pub async fn new(ctrl: FloControllerClient<Channel>, archiver: ArchiverHandle) -> Result<Self> {
    let swap_dir = tempdir()?;
    let mem_cache = MemCacheMgr::new(swap_dir.path().to_owned()).start();
    let listener = FloListener::bind_v4(flo_constants::OBSERVER_SOCKET_PORT).await?;
    Ok(Self {
      swap_dir,
      ctrl,
      archiver,
      mem_cache,
      listener,
    })
  }

  pub async fn serve(mut self) -> Result<()> {
    while let Some(transport) = self.listener.incoming().try_next().await? {
      let handler = Handler {
        ctrl: self.ctrl.clone(),
        archiver: self.archiver.clone(),
        mem_cache: self.mem_cache.addr(),
        transport,
      };
      tokio::spawn(handler.run());
    }
    Ok(())
  }
}

struct Handler {
  ctrl: FloControllerClient<Channel>,
  archiver: ArchiverHandle,
  mem_cache: Addr<MemCacheMgr>,
  transport: FloStream,
}

impl Handler {
  async fn run(mut self) {}

  async fn accept(&mut self) -> Result<Option<ObserverToken>> {
    use flo_grpc::controller::GetGameRequest;
    use flo_net::observer::{PacketObserverConnect, PacketObserverConnectAccept};
    let connect: PacketObserverConnect = self.transport.recv().await?;
    let token = match crate::token::validate_observer_token(&connect.token) {
      Ok(v) => v,
      Err(_) => {
        self
          .reject(ObserverConnectRejectReason::InvalidToken, None)
          .await?;
        return Ok(None);
      }
    };
    let game = match self
      .ctrl
      .get_game(GetGameRequest {
        game_id: token.game_id,
      })
      .await
    {
      Ok(game) => game.into_inner(),
      Err(err) => {
        tracing::error!(game_id = token.game_id, "get game: {}", err);
        self
          .reject(ObserverConnectRejectReason::GameNotReady, None)
          .await?;
        return Ok(None);
      }
    };
    let game = if let Some(game) = game.game {
      game
    } else {
      self
        .reject(ObserverConnectRejectReason::GameNoFound, None)
        .await?;
      return Ok(None);
    };

    let start_time = game.started_at.as_ref().map(|v| v.seconds).or_else(|| {
      use flo_grpc::game::GameStatus;
      match game.status() {
        GameStatus::Preparing | GameStatus::Created => None,
        GameStatus::Running | GameStatus::Ended | GameStatus::Paused | GameStatus::Terminated => {
          game.created_at.as_ref().map(|v| v.seconds)
        }
      }
    });

    let now = (SystemTime::now().duration_since(SystemTime::UNIX_EPOCH))
      .unwrap()
      .as_secs() as i64;
    let expected = if let Some(start_time) = start_time {
      start_time + (token.delay_secs.unwrap_or_default() as i64)
    } else {
      self
        .reject(ObserverConnectRejectReason::GameNotReady, None)
        .await?;
      return Ok(None);
    };

    if expected > now {
      self
        .reject(ObserverConnectRejectReason::DelayNotOver, expected.into())
        .await?;
      return Ok(None);
    }
    Ok(Some(token))
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
