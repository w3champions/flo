use backoff::{future::FutureOperation as _, ExponentialBackoff};
use futures::future::{abortable, AbortHandle};
use futures::FutureExt;
use parking_lot::RwLock;
use s2_grpc_utils::S2ProtoUnpack;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Notify;
use tokio::time::delay_for;
use tracing_futures::Instrument;

use flo_net::packet::*;
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;

use crate::error::*;
use crate::game::{Game, SlotStatus};
use crate::node::PlayerToken;

pub type HandlerSender = Sender<Frame>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct NodeConnRef {
  state: Arc<NodeConnState>,
}

impl NodeConnRef {
  pub fn new(addr: &str, secret: &str) -> Result<Self> {
    let (sender, receiver) = channel(1);

    let (ip, port) = parse_addr(addr)?;

    let state = Arc::new(NodeConnState {
      sender,
      secret: secret.to_string(),
      status: RwLock::new(NodeConnStatus::Connecting),
      dropper: Notify::new(),
      creating_games: RwLock::new(HashMap::new()),
    });

    tokio::spawn(
      {
        let state = state.clone();
        let addr = addr.to_string();
        async move {
          let worker = backoff::future::retry_notify(
            backoff::ExponentialBackoff {
              multiplier: 2_f64,
              ..backoff::ExponentialBackoff::default()
            },
            {
              let state = state.clone();
              move || Self::worker(state.clone(), ip, port)
            },
            {
              let state = state.clone();
              move |e, d| {
                tracing::error!("node conn: {}", e);
                tracing::debug!("retry after: {:?}", d);
                *state.status.write() = NodeConnStatus::Reconnecting;
              }
            },
          );
          tokio::select! {
            r = worker => {
              tracing::error!("worker returned unexpectedly: {:?}", r);
            },
            _ = state.dropper.notified() => {
              tracing::debug!("dropped");
            }
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("node conn worker", addr)),
    );

    Ok(Self { state })
  }

  pub async fn create_game(&self, game: &Game) -> Result<CreatedGameInfo> {
    let game_id = game.id;

    if self.state.creating_games.read().contains_key(&game_id) {
      return Err(Error::GameCreating);
    }

    let (tx, rx) = oneshot::channel();

    let pkt = PacketControllerCreateGame {
      game: Some(flo_net::proto::flo_node::Game {
        id: game_id,
        settings: Some(flo_net::proto::flo_node::GameSettings {
          map_path: game.map.path.clone(),
          map_sha1: game.map.sha1.to_vec(),
          map_checksum: game.map.checksum,
        }),
        slots: game
          .slots
          .iter()
          .enumerate()
          .filter_map(|(i, slot)| {
            if slot.settings.status == SlotStatus::Occupied {
              Some(flo_net::proto::flo_node::GameSlot {
                id: i as u32,
                player: slot.player.as_ref().map(|player| GamePlayer {
                  player_id: player.id,
                  name: player.name.clone(),
                }),
                settings: slot.settings.clone().into_packet().into(),
                client_status: Default::default(),
              })
            } else {
              None
            }
          })
          .collect(),
        status: Default::default(),
      }),
    };

    // handle timeout
    let (timeout, abort_timeout) = abortable({
      let state = self.state.clone();
      async move {
        delay_for(REQUEST_TIMEOUT).await;
        state.reply_create_game(game_id, Err(Error::GameCreateTimeout))
      }
    });

    let pending = CreatingGame {
      sender: Some(tx),
      abort_timeout: Some(abort_timeout),
    };

    self.state.creating_games.write().insert(game_id, pending);

    tokio::spawn({
      let mut sender = self.state.sender.clone();
      async move {
        tokio::time::timeout(REQUEST_TIMEOUT, async move {
          sender
            .send(pkt.encode_as_frame()?)
            .await
            .map_err(|_| Error::TaskCancelled)?;
          Ok::<_, Error>(())
        })
        .await??;
        Ok(())
      }
      .map({
        let state = self.state.clone();
        move |res: Result<_>| {
          if let Err(err) = res {
            state.reply_create_game(game_id, Err(err))
          }
        }
      })
    });

    let res = rx.await.map_err(|_| Error::TaskCancelled)??;
    Ok(res)
  }

  async fn worker(
    state: Arc<NodeConnState>,
    ip: Ipv4Addr,
    port: u16,
  ) -> Result<(), backoff::Error<Error>> {
    fn map_net_err(e: flo_net::error::Error) -> backoff::Error<Error> {
      backoff::Error::Transient(Error::from(e))
    }

    let addr = SocketAddrV4::new(ip, port);
    let mut stream = FloStream::connect(addr).await.map_err(map_net_err)?;

    stream
      .send(PacketControllerConnect {
        lobby_version: Some(crate::version::FLO_LOBBY_VERSION.into()),
        secret: state.secret.clone(),
      })
      .await
      .map_err(map_net_err)?;

    let res = stream.recv_frame().await.map_err(map_net_err)?;

    (flo_net::select_flo_packet! {
      res => {
        packet = PacketControllerConnectAccept => {
          tracing::debug!("node connected: version = {:?}", packet.version);
        }
        packet = PacketControllerConnectReject => {
          return Err(
            backoff::Error::Permanent(
              Error::NodeConnectionRejected {
                addr,
                reason: packet.reason(),
              }
            )
          )
        }
        packet = PacketControllerCreateGameAccept => {
          let game_id = packet.game_id;
          state.reply_create_game(
            game_id,
            CreatedGameInfo::unpack(packet).map_err(Into::into),
          )
        }
        packet = PacketControllerCreateGameReject => {
          let game_id = packet.game_id;
          state.reply_create_game(
            game_id,
            Err(Error::GameCreateReject(packet.reason())),
          )
        }
      }
    })
    .map_err(map_net_err)?;

    *state.status.write() = NodeConnStatus::Connected;

    loop {
      delay_for(Duration::from_secs(100)).await;
    }

    Ok(())
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::PacketControllerCreateGameAccept))]
pub struct CreatedGameInfo {
  pub game_id: i32,
  pub player_tokens: Vec<PlayerToken>,
}

impl S2ProtoUnpack<flo_net::proto::flo_node::PlayerToken> for PlayerToken {
  fn unpack(
    value: flo_net::proto::flo_node::PlayerToken,
  ) -> Result<Self, s2_grpc_utils::result::Error> {
    let mut bytes = [0_u8; 16];
    if value.id.len() >= 16 {
      bytes.clone_from_slice(&value.id[0..16]);
    } else {
      (&mut bytes[0..(value.id.len())]).clone_from_slice(&value.id[0..(value.id.len())]);
    }
    Ok(PlayerToken(bytes))
  }
}

#[derive(Debug)]
struct CreatingGame {
  sender: Option<oneshot::Sender<Result<CreatedGameInfo>>>,
  abort_timeout: Option<AbortHandle>,
}

impl Drop for CreatingGame {
  fn drop(&mut self) {
    if let Some(handle) = self.abort_timeout.take() {
      handle.abort()
    }
  }
}

#[derive(Debug)]
struct NodeConnState {
  sender: Sender<Frame>,
  secret: String,
  status: RwLock<NodeConnStatus>,
  dropper: Notify,
  creating_games: RwLock<HashMap<i32, CreatingGame>>,
}

impl NodeConnState {
  fn reply_create_game(&self, game_id: i32, result: Result<CreatedGameInfo>) {
    let sender = self
      .creating_games
      .write()
      .remove(&game_id)
      .and_then(|mut v| v.sender.take());
    if let Some(sender) = sender {
      sender.send(result).ok();
    } else {
      tracing::warn!("failed to reply: sender missing: {:?}", result);
    }
  }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum NodeConnStatus {
  Connecting,
  Connected,
  Reconnecting,
}

fn parse_addr(addr: &str) -> Result<(Ipv4Addr, u16)> {
  let (ip, port) = if addr.contains(":") {
    let addr = if let Some(addr) = addr.parse::<SocketAddrV4>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeAddress(addr.to_string()));
    };

    (addr.ip().clone(), addr.port())
  } else {
    let addr: Ipv4Addr = if let Some(addr) = addr.parse::<Ipv4Addr>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeAddress(addr.to_string()));
    };
    let port = flo_constants::NODE_CONTROLLER_PORT;
    (addr, port)
  };
  Ok((ip, port))
}
