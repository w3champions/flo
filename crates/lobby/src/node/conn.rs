use parking_lot::RwLock;
use s2_grpc_utils::S2ProtoUnpack;
use std::collections::HashMap;
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_net::packet::*;
use flo_net::stream::FloStream;

use crate::error::*;
use crate::game::Game;
use crate::node::PlayerToken;

pub type HandlerSender = Sender<Frame>;

#[derive(Debug)]
pub struct NodeConn {
  sender: Sender<Frame>,
  connected: Arc<AtomicBool>,
  dropper: Arc<Notify>,
  creating_games: Arc<RwLock<HashMap<i32, CreatingGame>>>,
}

impl NodeConn {
  pub fn new() -> Self {
    let (sender, receiver) = channel(1);
    let connected = Arc::new(AtomicBool::new(false));
    let dropper = Arc::new(Notify::new());

    tokio::spawn(
      {
        let dropper = dropper.clone();
        let connected = connected.clone();
        async move { loop {} }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    Self {
      sender,
      connected,
      dropper,
      creating_games: Arc::new(RwLock::new(HashMap::new())),
    }
  }

  pub async fn create_game(&self) -> Result<CreateGameResponse> {
    Ok(CreateGameResponse::Timeout)
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
