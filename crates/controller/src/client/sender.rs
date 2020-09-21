use flo_net::packet::*;
use flo_net::proto::flo_connect::*;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::error::*;

pub type PlayerReceiver = Receiver<PlayerSenderMessage>;
pub enum PlayerSenderMessage {
  Frame(Frame),
  Disconnect(ClientDisconnectReason),
}

#[derive(Debug, Clone)]
pub struct PlayerSender {
  player_id: i32,
  sender: Sender<PlayerSenderMessage>,
}

impl PlayerSender {
  pub fn new(player_id: i32) -> (Self, PlayerReceiver) {
    let (sender, receiver) = channel(8);
    (PlayerSender { player_id, sender }, receiver)
  }

  pub fn player_id(&self) -> i32 {
    self.player_id
  }

  pub async fn disconnect_multi(&mut self) {
    self.disconnect(ClientDisconnectReason::Multi).await;
  }

  #[tracing::instrument]
  async fn disconnect(&mut self, reason: ClientDisconnectReason) {
    self
      .sender
      .send_timeout(
        PlayerSenderMessage::Disconnect(reason),
        Duration::from_secs(3),
      )
      .await
      .ok();
  }

  pub fn try_send(&mut self, frame: Frame) -> bool {
    self
      .sender
      .try_send(PlayerSenderMessage::Frame(frame))
      .is_ok()
  }

  pub async fn send_frame(&mut self, frame: Frame) -> Result<()> {
    self
      .sender
      .send(PlayerSenderMessage::Frame(frame))
      .await
      .map_err(|_| Error::PlayerStreamClosed)?;
    Ok(())
  }

  pub async fn send<T: FloPacket>(&mut self, packet: T) -> Result<()> {
    self.send_frame(packet.encode_as_frame()?).await?;
    Ok(())
  }
}
