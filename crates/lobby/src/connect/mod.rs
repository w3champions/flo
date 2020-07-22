use futures::stream::TryStream;

use flo_net::connect;
use flo_net::listener::FloListener;
use flo_net::stream::FloStream;

use crate::db::ExecutorRef;
use crate::error::Result;
use crate::state::LobbyStateRef;

mod handshake;
mod state;
pub use state::NotificationSender;
use tokio::stream::StreamExt;

pub async fn serve(state: LobbyStateRef) -> Result<()> {
  let mut listener = FloListener::bind_v4(crate::constants::LOBBY_SOCKET_PORT).await?;
  tracing::info!("listening on port {}", listener.port());

  while let Some(stream) = listener.incoming().try_next().await? {
    let state = state.clone();
    tokio::spawn(async move {
      if let Err(err) = handle_stream(state, stream).await {
        tracing::warn!("stream error: {}", err);
      }
    });
  }

  tracing::info!("shutting down");

  Ok(())
}

async fn handle_stream(state: LobbyStateRef, mut stream: FloStream) -> Result<()> {
  tracing::debug!("connected: {}", stream.peer_addr()?);

  let accepted = match handshake::handle_handshake(&mut stream).await {
    Ok(accepted) => accepted,
    Err(e) => {
      tracing::debug!("dropping: handshake error: {}", e);
      return Ok(());
    }
  };

  let player_id = accepted.player_id;
  tracing::debug!("accepted: player_id = {}", player_id);

  let player = state
    .db
    .exec(move |conn| crate::player::db::get_ref(conn, player_id))
    .await?;

  let game_id = {
    let player = state.mem.lock_player_state(player.id).await;
    player.joined_game_id()
  };

  stream
    .send(connect::PacketConnectLobbyAccept {
      lobby_version: Some(From::from(crate::version::FLO_LOBBY_VERSION)),
      session: Some({
        use flo_net::proto::flo_connect::*;
        Session {
          player: Some(PlayerInfo {
            id: player.id,
            name: player.name,
            source: player.source as i32,
          }),
          status: if game_id.is_some() {
            PlayerStatus::InGame.into()
          } else {
            PlayerStatus::Idle.into()
          },
          game_id,
        }
      }),
    })
    .await?;

  tokio::time::delay_for(std::time::Duration::from_secs(1000)).await;

  tracing::debug!("dropping: player_id = {}", player_id);

  Ok(())
}
