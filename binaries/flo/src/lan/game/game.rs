use tokio::sync::mpsc::Receiver;
use tokio::sync::watch::Receiver as WatchReceiver;

use flo_w3gs::net::W3GSStream;
use flo_w3gs::packet::*;
use flo_w3gs::protocol::chat::ChatToHost;
use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};

use crate::error::*;
use crate::lan::game::LanGameInfo;
use crate::node::stream::NodeStreamHandle;
use crate::types::{NodeGameStatus, SlotClientStatus};

#[derive(Debug)]
pub enum GameResult {
  Disconnected,
  Leave,
}

#[derive(Debug)]
pub struct GameHandler<'a> {
  info: &'a LanGameInfo,
  stream: &'a mut W3GSStream,
  node_stream: &'a mut NodeStreamHandle,
  status_rx: &'a mut WatchReceiver<Option<NodeGameStatus>>,
  w3gs_rx: &'a mut Receiver<Packet>,
}

impl<'a> GameHandler<'a> {
  pub fn new(
    info: &'a LanGameInfo,
    stream: &'a mut W3GSStream,
    node_stream: &'a mut NodeStreamHandle,
    status_rx: &'a mut WatchReceiver<Option<NodeGameStatus>>,
    w3gs_rx: &'a mut Receiver<Packet>,
  ) -> Self {
    GameHandler {
      info,
      stream,
      node_stream,
      status_rx,
      w3gs_rx,
    }
  }

  pub async fn run(&mut self) -> Result<GameResult> {
    let mut loop_state = GameLoopState::new(&self.info);

    loop {
      tokio::select! {
        next = self.stream.recv() => {
          let pkt = match next {
            Ok(pkt) => pkt,
            Err(err) => {
              tracing::error!("game connection: {}", err);
              return Ok(GameResult::Disconnected)
            },
          };
          if let Some(pkt) = pkt {
            // tracing::debug!("game => {:?}", pkt.type_id());
            if pkt.type_id() == LeaveReq::PACKET_TYPE_ID {
              self.node_stream.report_slot_status(SlotClientStatus::Left).await.ok();
              self.stream.send(Packet::simple(LeaveAck)?).await?;
              self.stream.flush().await?;
              return Ok(GameResult::Leave)
            }

            self.handle_packet(&mut loop_state, pkt).await?;
          } else {
            return Ok(GameResult::Disconnected)
          }
        }
        next = self.status_rx.recv() => {
          let next = if let Some(next) = next {
            next
          } else {
            return Err(Error::TaskCancelled)
          };
          match next {
            Some(status) => {
              self.handle_game_status_change(&mut loop_state, status).await?;
            },
            None => {},
          }
        }
        next = self.w3gs_rx.recv() => {
          if let Some(pkt) = next {
            self.handle_w3gs(&mut loop_state, pkt).await?;
          } else {
            return Err(Error::TaskCancelled)
          }
        }
      }
    }
  }

  #[inline]
  async fn handle_w3gs(&mut self, _state: &mut GameLoopState, pkt: Packet) -> Result<()> {
    // tracing::debug!("game <= {:?}", pkt.type_id());
    self.stream.send(pkt).await?;
    Ok(())
  }

  async fn handle_game_status_change(
    &mut self,
    _state: &mut GameLoopState,
    status: NodeGameStatus,
  ) -> Result<()> {
    tracing::debug!("game status changed: {:?}", status);
    Ok(())
  }

  async fn handle_packet(&mut self, _state: &mut GameLoopState, pkt: Packet) -> Result<()> {
    match pkt.type_id() {
      ChatToHost::PACKET_TYPE_ID => {
        // TODO: implement commands
        // self
        //   .stream
        //   .send(Packet::simple(ChatFromHost::chat(
        //     slot_info.slot_player_id,
        //     &[slot_info.slot_player_id],
        //     "Setting changes and chat are disabled.",
        //   ))?)
        //   .await?;
      }
      _ => {}
    }

    self.node_stream.send_w3gs(pkt).await?;

    Ok(())
  }
}

#[derive(Debug)]
struct GameLoopState {
  time: u32,
  ping: Option<u32>,
}

impl GameLoopState {
  fn new(_info: &LanGameInfo) -> Self {
    GameLoopState {
      time: 0,
      ping: None,
    }
  }
}
