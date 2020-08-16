mod error;
mod event;
mod lobby;
mod node;
mod platform;
mod version;

use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::lobby::Lobby;
use crate::platform::PlatformState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log::init_env("flo=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let platform = PlatformState::init().await?.into_ref();
  let (lobby_sender, mut lobby_receiver) = channel(1);
  let lobby = Lobby::init(platform.clone(), lobby_sender).await;
  tokio::spawn(async move {
    while let Some(event) = lobby_receiver.recv().await {
      dbg!(event);
    }
  });

  tokio::signal::ctrl_c().await?;

  Ok(())
}
