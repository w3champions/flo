mod error;
mod lobby;
mod node;
mod platform;
mod version;

use tokio::sync::mpsc::channel;

use crate::lobby::{GameStartedEvent, Lobby, LobbyEvent};
use crate::platform::PlatformState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log::init_env("flo=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let platform = PlatformState::init().await?.into_ref();
  let (lobby_sender, mut lobby_receiver) = channel(1);
  let _lobby = Lobby::init(platform.clone(), lobby_sender).await;
  tokio::spawn(async move {
    while let Some(event) = lobby_receiver.recv().await {
      match event {
        LobbyEvent::WsWorkerErrorEvent(err) => {
          tracing::error!("websocket: {}", err);
        }
        LobbyEvent::GameStartedEvent(event) => {
          tracing::info!(
            game_id = event.game_id,
            "node token = {:?}",
            event.player_token
          );
        }
      }
    }
  });

  tokio::signal::ctrl_c().await?;

  Ok(())
}
