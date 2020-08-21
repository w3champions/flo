mod error;
mod lan;
mod lobby;
mod node;
mod platform;
mod version;
use tokio::sync::mpsc::channel;

use flo_event::FloEvent;

use crate::lan::{Lan, LanEvent};
use crate::lobby::{GameStartedEvent, Lobby, LobbyEvent};
use crate::platform::PlatformState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log::init_env("flo=debug,flo_lan=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let platform = PlatformState::init().await?.into_ref();
  let (lobby_sender, mut lobby_receiver) = channel(1);
  let lobby = Lobby::init(platform.clone(), lobby_sender).await?;
  let (lan_sender, mut lan_receiver) = LanEvent::channel(1);
  let lan = Lan::new(lan_sender.into());
  tokio::spawn({
    let lobby = lobby.handle();
    let lan = lan.handle();
    async move {
      loop {
        tokio::select! {
          next = lobby_receiver.recv() => {
            if let Some(event) = next {
              match event {
                LobbyEvent::ConnectedEvent => {},
                LobbyEvent::DisconnectedEvent => {
                  lan.stop_game();
                },
                LobbyEvent::GameInfoUpdateEvent(game_info) => {
                  match game_info {
                    Some(game_info) => {
                      tracing::info!(game_id = game_info.game_id, "joined game: {:?}", game_info);
                    },
                    None => {
                      lan.stop_game();
                    },
                  }
                },
                LobbyEvent::GameStartedEvent(event, game_info) => {
                  tracing::info!(
                    game_id = event.game_id,
                    "node token = {:?}, game = {:?}",
                    event.player_token,
                    game_info
                  );
                  if let Some(my_player_id) = lobby.with_player_session(|s| s.player.id) {
                    flo_log::result_ok!(
                      "replace lan game",
                      lan.replace_game(my_player_id, game_info).await
                    );
                  } else {
                    tracing::error!("received GameStartedEvent but there is no active player session.");
                  }
                },
                LobbyEvent::WsWorkerErrorEvent(err) => {
                  tracing::error!("websocket: {}", err);
                }
              }
            } else {
              break;
            }
          }
          next = lan_receiver.recv() => {
            if let Some(event) = next {
              match event {
                event => {
                  dbg!(&event);
                }
              }
            } else {
              break;
            }
          }
        }
      }
    }
  });

  tokio::signal::ctrl_c().await?;

  Ok(())
}
