use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::watch::Receiver as WatchReceiver;

use flo_w3gs::net::W3GSStream;
use flo_w3gs::packet::*;
use flo_w3gs::protocol::action::{IncomingAction, OutgoingAction, OutgoingKeepAlive};
use flo_w3gs::protocol::chat::{ChatMessage, ChatToHost};
use flo_w3gs::protocol::leave::LeaveAck;

use crate::error::*;
use crate::lan::game::proxy::PlayerEvent;
use crate::lan::game::LanGameInfo;
use crate::node::stream::NodeStreamHandle;
use crate::node::NodeInfo;
use crate::types::{NodeGameStatus, SlotClientStatus};
use flo_util::chat::parse_chat_command;
use flo_w3gs::chat::ChatFromHost;

#[derive(Debug)]
pub enum GameResult {
  Disconnected,
  Leave,
}

#[derive(Debug)]
pub struct GameHandler<'a> {
  info: &'a LanGameInfo,
  node: &'a NodeInfo,
  w3gs_stream: &'a mut W3GSStream,
  node_stream: &'a mut NodeStreamHandle,
  event_rx: &'a mut Receiver<PlayerEvent>,
  status_rx: &'a mut WatchReceiver<Option<NodeGameStatus>>,
  w3gs_tx: &'a mut Sender<Packet>,
  w3gs_rx: &'a mut Receiver<Packet>,
  tick_recv: u32,
  tick_ack: u32,
}

impl<'a> GameHandler<'a> {
  pub fn new(
    info: &'a LanGameInfo,
    node: &'a NodeInfo,
    stream: &'a mut W3GSStream,
    node_stream: &'a mut NodeStreamHandle,
    event_rx: &'a mut Receiver<PlayerEvent>,
    status_rx: &'a mut WatchReceiver<Option<NodeGameStatus>>,
    w3gs_tx: &'a mut Sender<Packet>,
    w3gs_rx: &'a mut Receiver<Packet>,
  ) -> Self {
    GameHandler {
      info,
      node,
      w3gs_stream: stream,
      node_stream,
      event_rx,
      status_rx,
      w3gs_tx,
      w3gs_rx,
      tick_recv: 0,
      tick_ack: 0,
    }
  }

  pub async fn run(&mut self) -> Result<GameResult> {
    let mut loop_state = GameLoopState::new(&self.info);

    loop {
      tokio::select! {
        next = self.w3gs_stream.recv() => {
          let pkt = match next {
            Ok(pkt) => pkt,
            Err(err) => {
              tracing::error!("game connection: {}", err);
              return Ok(GameResult::Disconnected)
            },
          };
          if let Some(pkt) = pkt {
            // tracing::debug!("game => {:?}", pkt.type_id());
            if pkt.type_id() == LeaveAck::PACKET_TYPE_ID {
              self.node_stream.report_slot_status(SlotClientStatus::Left).await.ok();
              self.w3gs_stream.send(Packet::simple(LeaveAck)?).await?;
              self.w3gs_stream.flush().await?;
              return Ok(GameResult::Leave)
            }

            self.handle_game_packet(&mut loop_state, pkt).await?;
          } else {
            tracing::error!("stream closed");
            return Ok(GameResult::Disconnected)
          }
        }
        next = self.event_rx.recv() => {
          match next {
            Some(PlayerEvent::PlayerStatusChange {
              player_id,
              status
            }) => {
              tracing::debug!(game_id = self.info.game.game_id, player_id, "slot client status change: {:?}", status);
            },
            None => {}
          }
        }
        next = self.status_rx.recv() => {
          let next = if let Some(next) = next {
            next
          } else {
            return Err(Error::TaskCancelled(anyhow::format_err!("game status tx dropped")))
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
            self.handle_incoming_w3gs(&mut loop_state, pkt).await?;
          } else {
            return Err(Error::TaskCancelled(anyhow::format_err!("w3g tx dropped")))
          }
        }
      }
    }
  }

  #[inline]
  async fn handle_incoming_w3gs(&mut self, _state: &mut GameLoopState, pkt: Packet) -> Result<()> {
    match pkt.type_id() {
      OutgoingKeepAlive::PACKET_TYPE_ID => {}
      IncomingAction::PACKET_TYPE_ID => {
        self.tick_recv += 1;
      }
      OutgoingAction::PACKET_TYPE_ID => {}
      _ => {}
    }

    self.w3gs_stream.send(pkt).await?;
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

  async fn handle_game_packet(&mut self, _state: &mut GameLoopState, pkt: Packet) -> Result<()> {
    match pkt.type_id() {
      ChatToHost::PACKET_TYPE_ID => {
        let pkt: ChatToHost = pkt.decode_simple()?;
        match pkt.message {
          ChatMessage::Scoped { message, .. } => {
            if let Some(cmd) = parse_chat_command(message.as_bytes()) {
              self.handle_chat_command(&cmd);
              return Ok(());
            }
          }
          _ => {}
        }
      }
      OutgoingKeepAlive::PACKET_TYPE_ID => self.tick_ack += 1,
      IncomingAction::PACKET_TYPE_ID => {}
      OutgoingAction::PACKET_TYPE_ID => {}
      _ => {
        tracing::debug!("unknown game packet: {:?}", pkt.type_id());
      }
    }

    self.node_stream.send_w3gs(pkt).await?;

    Ok(())
  }

  fn handle_chat_command(&mut self, cmd: &str) {
    match cmd.trim_end() {
      "flo" => {
        let mut messages = vec![
          format!(
            "Game: {} (#{})",
            self.info.game.name, self.info.game.game_id
          ),
          format!(
            "Server: {}, {}, {} (#{})",
            self.node.name, self.node.location, self.node.country_id, self.node.id
          ),
          "Players:".to_string(),
        ];

        for slot in &self.info.game.slots {
          if let Some(ref player) = slot.player.as_ref() {
            messages.push(format!(
              "  {}: Team {}, {:?}",
              player.name, slot.settings.team, slot.settings.race
            ));
          }
        }

        self.send_chats_to_self(self.info.slot_info.slot_player_id, messages)
      }
      "tick" => self.send_chats_to_self(
        self.info.slot_info.slot_player_id,
        vec![format!(
          "tick_recv = {}, tick_ack = {}",
          self.tick_recv, self.tick_ack
        )],
      ),
      _ => self.send_chats_to_self(
        self.info.slot_info.slot_player_id,
        vec![format!("Unknown command")],
      ),
    }
  }

  fn send_chats_to_self(&self, player_id: u8, messages: Vec<String>) {
    let mut tx = self.w3gs_tx.clone();
    tokio::spawn(async move {
      for message in messages {
        match Packet::simple(ChatFromHost::private_to_self(player_id, message)) {
          Ok(pkt) => {
            tx.send(pkt).await.ok();
          }
          Err(err) => {
            tracing::error!("encode chat packet: {}", err);
          }
        }
      }
    });
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
