mod controller;
mod error;
mod lan;
mod node;
mod platform;
mod types;
mod version;
use tokio::sync::mpsc::channel;

use flo_event::FloEvent;

use crate::controller::{ControllerClient, ControllerEvent, GameStartedEvent};
use crate::lan::{Lan, LanEvent};
use crate::platform::PlatformState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log_subscriber::init_env("flo=debug,flo_lan=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let platform = PlatformState::init().await?.into_ref();
  let (ctrl_sender, mut ctrl_receiver) = channel(1);
  let ctrl = ControllerClient::init(platform.clone(), ctrl_sender).await?;
  let (lan_sender, mut lan_receiver) = LanEvent::channel(1);
  let lan = Lan::new(platform.clone(), lan_sender.into());
  tokio::spawn({
    let ctrl = ctrl.handle();
    let lan = lan.handle();
    async move {
      loop {
        tokio::select! {
          next = ctrl_receiver.recv() => {
            if let Some(event) = next {
              match event {
                ControllerEvent::ConnectedEvent => {},
                ControllerEvent::DisconnectedEvent => {
                  lan.stop_game();
                },
                ControllerEvent::GameInfoUpdateEvent(game_info) => {
                  match game_info {
                    Some(game_info) => {
                      tracing::info!(game_id = game_info.game_id, "game info update");
                    },
                    None => {
                      lan.stop_game();
                    },
                  }
                },
                ControllerEvent::GameStartedEvent(event, game_info) => {
                  if let Some(my_player_id) = ctrl.with_player_session(|s| s.player.id) {
                    flo_log::result_ok!(
                      "replace lan game",
                      lan.replace_game(my_player_id, game_info).await
                    );
                  } else {
                    tracing::error!("received GameStartedEvent but there is no active player session.");
                  }
                },
                ControllerEvent::WsWorkerErrorEvent(err) => {
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
