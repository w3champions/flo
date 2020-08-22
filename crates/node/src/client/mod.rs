mod event;
pub use event::{ClientEvent, ClientEventSender};

use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_constants::NODE_CLIENT_PORT;
use flo_net::listener::FloListener;
use flo_net::packet::{FloPacket, Frame};
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;
use flo_net::try_flo_packet;

use crate::error::*;
use crate::session::{PlayerToken, SessionStore};

pub async fn serve_client() -> Result<()> {
  let mut listener = FloListener::bind_v4(NODE_CLIENT_PORT).await?;

  while let Some(incoming) = listener.incoming().next().await {
    if let Ok(mut stream) = incoming {
      tokio::spawn(async move {
        let state = match handshake(&mut stream).await {
          Ok(state) => state,
          Err(err) => {
            let reason = match &err {
              Error::InvalidToken => ClientConnectRejectReason::InvalidToken,
              _ => ClientConnectRejectReason::Unknown,
            };
            stream
              .send(PacketClientConnectReject {
                reason: reason.into(),
                message: format!("{}", err),
              })
              .await
              .ok();
            return;
          }
        };

        tracing::debug!(
          game_id = state.game_id,
          player_id = state.player_id,
          "connected"
        );
      });
    }
  }

  Ok(())
}

async fn handshake(stream: &mut FloStream) -> Result<ClientState> {
  use std::time::Duration;
  const RECV_TIMEOUT: Duration = Duration::from_secs(3);

  let connect: PacketClientConnect = stream.recv_timeout(RECV_TIMEOUT).await?;

  let session = SessionStore::get();

  let token = if let Some(token) = PlayerToken::from_vec(connect.token) {
    token
  } else {
    return Err(Error::InvalidToken);
  };

  let pending = session
    .get_pending_player(&token)
    .ok_or_else(|| Error::InvalidToken)?;

  Ok(ClientState {
    game_id: pending.game_id,
    player_id: pending.player_id,
  })
}

#[derive(Debug)]
pub struct ClientState {
  game_id: i32,
  player_id: i32,
}

#[derive(Debug, Clone)]
pub struct PlayerSender {
  sender: Sender<Frame>,
}

#[derive(Debug)]
pub struct PlayerReceiver {
  receiver: Receiver<Frame>,
}
