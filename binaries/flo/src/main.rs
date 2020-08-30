mod controller;
mod error;
mod lan;
mod node;
mod platform;
mod types;
mod version;
use tokio::sync::mpsc::channel;

use flo_event::FloEvent;

use crate::controller::{ControllerClient, ControllerEvent};
use crate::lan::{Lan, LanEvent};
use crate::node::stream::NodeStreamEvent;
use crate::platform::PlatformState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log_subscriber::init_env("flo=debug,flo_lan=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let platform = PlatformState::init().await?.into_ref();
  let (ctrl_sender, mut ctrl_receiver) = channel(3);
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
                ControllerEvent::DisconnectedEvent => {},
                ControllerEvent::GameInfoUpdateEvent(game_info) => {
                  match game_info {
                    Some(game_info) => {
                      tracing::debug!(game_id = game_info.game_id, "game info update");
                    },
                    None => {
                      lan.stop_game().await;
                    },
                  }
                },
                ControllerEvent::GameStartedEvent(event, game_info) => {
                  if let Some(node_info) = ctrl.get_node(event.node_id) {
                    if let Some(my_player_id) = ctrl.with_player_session(|s| s.player.id) {
                      flo_log::result_ok!(
                        "update lan game",
                        lan.replace_game(my_player_id, node_info, event.player_token, game_info).await
                      );
                      // FIXME: report error!
                    } else {
                      tracing::error!("received GameStartedEvent but there is no active player session.");
                    }
                  } else {
                    tracing::error!("unknown node id: {}", event.node_id);
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
                LanEvent::NodeStreamEvent(event) => {
                  match event {
                    NodeStreamEvent::SlotClientStatusUpdate(update) => {
                      ctrl.ws_send_update_slot_client_status(update.clone()).await;
                      if let Err(err) = lan.update_player_status(update.game_id, update.player_id, update.status).await {
                        tracing::error!(game_id = update.game_id, player_id = update.player_id, "update lan player status to {:?}: {}", update.status, err);
                      }
                    },
                    NodeStreamEvent::GameInitialStatus(data) => {
                      if let Err(err) = lan.update_game_status(data.game_id, data.game_status, &data.player_game_client_status_map).await {
                        tracing::error!(game_id = data.game_id, "update lan game status to {:?}: {}", data.game_status, err);
                      }
                    },
                    NodeStreamEvent::GameStatusUpdate(update) => {
                      let game_id = update.game_id;
                      let game_status = update.status;
                      ctrl.ws_send_update_game_status(update.clone()).await;
                      if let Err(err) = lan.update_game_status(game_id, game_status, &update.updated_player_game_client_status_map).await {
                        tracing::error!(game_id, "update lan game status to {:?}: {}", game_status, err);
                      }
                    },
                    NodeStreamEvent::Disconnected => {
                      lan.stop_game().await;
                    }
                  }
                }
                LanEvent::GameDisconnected => {
                  lan.stop_game().await;
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
