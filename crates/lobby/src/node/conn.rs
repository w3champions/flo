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
use flo_net::stream::FloStream;

use crate::error::*;
use crate::game::Game;
use crate::node::PlayerToken;

pub type HandlerSender = Sender<Frame>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_AFTER: Duration = Duration::from_secs(5);
const RECONNECT_BACKOFF_RESET_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct NodeConn {
  sender: Sender<Frame>,
  state: Arc<NodeConnState>,
}

impl NodeConn {
  pub fn new(addr: &str) -> Result<Self> {
    let (sender, receiver) = channel(1);

    let (ip, port) = parse_addr(addr)?;

    let state = Arc::new(NodeConnState {
      status: RwLock::new(NodeConnStatus::Connecting),
      dropper: Notify::new(),
      creating_games: RwLock::new(HashMap::new()),
    });

    tokio::spawn(
      {
        let state = state.clone();
        let addr = addr.to_string();
        async move {
          let mut last_err_time: Option<Instant> = None;
          let mut backoff = RECONNECT_AFTER;
          loop {
            if let Err(e) = Self::worker(state.clone(), ip, port).await {
              tracing::error!(addr = &addr as &str, "node conn: {}", e);
              let now = Instant::now();
              if let Some(t) = last_err_time.take() {
                if let Some(d) = now.checked_duration_since(t) {
                  if d > RECONNECT_BACKOFF_RESET_AFTER {
                    backoff = RECONNECT_AFTER;
                  } else {
                    backoff = backoff * 2;
                  }
                }
              }
              last_err_time = Some(Instant::now());
            }
            tracing::debug!("reconnect backoff: {:?}", backoff);
            *state.status.write() = NodeConnStatus::Reconnecting;
            delay_for(RECONNECT_AFTER).await
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    Ok(Self { sender, state })
  }

  pub async fn create_game(&self) -> Result<CreateGameResponse> {
    Ok(CreateGameResponse::Timeout)
  }

  async fn worker(state: Arc<NodeConnState>, ip: Ipv4Addr, port: u16) -> Result<()> {
    *state.status.write() = NodeConnStatus::Connected;

    Ok(())
  }
}

#[derive(Debug)]
pub enum CreateGameResponse {
  Created(CreatedGameInfo),
  Timeout,
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
  sender: oneshot::Sender<CreateGameResponse>,
}

#[derive(Debug)]
struct NodeConnState {
  status: RwLock<NodeConnStatus>,
  dropper: Notify,
  creating_games: RwLock<HashMap<i32, CreatingGame>>,
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
    let port = flo_constants::NODE_ECHO_PORT;
    (addr, port)
  };
  Ok((ip, port))
}
