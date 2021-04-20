use futures::stream::StreamExt;

use flo_constants::NODE_CLIENT_PORT;
use flo_net::listener::FloListener;
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;

use crate::error::*;
use crate::state::{GlobalState, GlobalStateRef, PlayerToken};

pub async fn serve_client(state: GlobalStateRef) -> Result<()> {
  let mut listener = FloListener::bind_v4(NODE_CLIENT_PORT).await?;

  while let Some(incoming) = listener.incoming().next().await {
    if let Ok(mut stream) = incoming {
      let state = state.clone();
      tokio::spawn(async move {
        let claim = match handshake(&state, &mut stream).await {
          Ok(claim) => claim,
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
          game_id = claim.game_id,
          player_id = claim.player_id,
          "connected"
        );

        let session = match state.get_game(claim.game_id) {
          Some(session) => session,
          None => {
            stream
              .send(PacketClientConnectReject {
                reason: ClientConnectRejectReason::Unknown.into(),
                message: format!("Game session was not found."),
              })
              .await
              .ok();
            return;
          }
        };

        if let Err((stream, err)) = session
          .register_player_stream(claim.player_id, stream)
          .await
        {
          tracing::error!(
            game_id = claim.game_id,
            player_id = claim.player_id,
            "register player stream: {}",
            err
          );
          if let Some(mut stream) = stream {
            stream
              .send(PacketClientConnectReject {
                reason: match err {
                  Error::PlayerConnectionExists => ClientConnectRejectReason::Multi,
                  _ => ClientConnectRejectReason::Unknown,
                }
                .into(),
                message: format!("Register: {}", err),
              })
              .await
              .ok();
          }
        }
      });
    }
  }

  Ok(())
}

async fn handshake(state: &GlobalState, stream: &mut FloStream) -> Result<Claim> {
  use std::time::Duration;
  const RECV_TIMEOUT: Duration = Duration::from_secs(3);

  let connect: PacketClientConnect = stream.recv_timeout(RECV_TIMEOUT).await?;

  let token = if let Some(token) = PlayerToken::from_vec(connect.token) {
    token
  } else {
    return Err(Error::InvalidToken);
  };

  let pending = state
    .get_pending_player(&token)
    .ok_or_else(|| Error::InvalidToken)?;

  Ok(Claim {
    game_id: pending.game_id,
    player_id: pending.player_id,
  })
}

#[derive(Debug)]
pub struct Claim {
  game_id: i32,
  player_id: i32,
}
