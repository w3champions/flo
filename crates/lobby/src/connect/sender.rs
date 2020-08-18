use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use flo_net::packet::*;
use flo_net::proto::flo_connect::*;

use crate::error::*;

pub type PlayerReceiver = Receiver<Frame>;

#[derive(Debug, Clone)]
pub struct PlayerSender {
  sid: u64,
  player_id: i32,
  sender: Sender<Frame>,
}

impl PlayerSender {
  pub fn new(sid: u64, player_id: i32) -> (Self, PlayerReceiver) {
    let (sender, receiver) = channel(5);

    (
      PlayerSender {
        sid,
        player_id,
        sender,
      },
      receiver,
    )
  }

  pub fn player_id(&self) -> i32 {
    self.player_id
  }

  pub async fn disconnect_multi(&mut self) {
    self.disconnect(LobbyDisconnectReason::Multi).await.ok();
  }

  #[tracing::instrument]
  async fn disconnect(&mut self, reason: LobbyDisconnectReason) -> Result<()> {
    self
      .send(PacketLobbyDisconnect {
        reason: reason.into(),
      })
      .await?;
    Ok(())
  }

  pub async fn send_frame(&mut self, frame: Frame) -> Result<()> {
    self
      .sender
      .send(frame)
      .await
      .map_err(|_| Error::PlayerStreamClosed)?;
    Ok(())
  }

  pub async fn send<T: FloPacket>(&mut self, packet: T) -> Result<()> {
    self.send_frame(packet.encode_as_frame()?).await?;
    Ok(())
  }

  pub fn sid(&self) -> u64 {
    self.sid
  }
}

#[derive(Debug)]
pub struct PlayerBroadcaster {
  senders: Vec<PlayerSender>,
}

impl PlayerBroadcaster {
  pub fn empty() -> Self {
    Self { senders: vec![] }
  }

  pub fn new(senders: Vec<PlayerSender>) -> Self {
    Self { senders }
  }

  // Broadcasts a packet, ignore peer errors
  pub async fn broadcast<T>(self, pkt: T) -> Result<()>
  where
    T: FloPacket,
  {
    if self.senders.is_empty() {
      return Ok(());
    }

    let frame = pkt.encode_as_frame()?;
    let futures: FuturesUnordered<_> = self
      .senders
      .into_iter()
      .map(|mut sender| {
        let frame = frame.clone();
        async move {
          let player_id = sender.player_id;
          sender
            .send_frame(frame)
            .map(move |res| (player_id, res))
            .await
        }
      })
      .collect();
    let results: Vec<_> = futures.collect().await;
    for (player_id, res) in results {
      if let Err(_) = res {
        tracing::debug!(player_id, "frame discarded");
      }
    }
    Ok(())
  }
}
