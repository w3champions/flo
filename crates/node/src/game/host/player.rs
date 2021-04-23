use crate::error::Result;
use crate::game::host::stream::PlayerStreamHandle;
use crate::game::{PlayerBanType, PlayerSlot};
use flo_net::packet::Frame;
use flo_net::w3gs::{W3GSAckQueue, W3GSFrameExt, W3GSMetadata, W3GSPacket};
use flo_w3gs::protocol::chat::ChatFromHost;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::error::TrySendError;

#[derive(Debug)]
pub struct PlayerDispatchInfo {
  player_name: String,
  last_stream_id: Option<u64>,
  tx: Option<PlayerStreamHandle>,
  ban_list: Vec<PlayerBanType>,
  slot_player_id: u8,
  w3gs_ack_q: W3GSAckQueue,
  lag_duration_ms: u32,
  lag_start: Option<Instant>,
  lag_slot_ids: BTreeSet<u8>,
  delay: Option<Duration>,
  action_rtt: Option<Duration>,
}

impl PlayerDispatchInfo {
  pub fn new(slot: &PlayerSlot) -> Self {
    Self {
      player_name: slot.player.name.clone(),
      last_stream_id: None,
      tx: None,
      ban_list: slot.player.ban_list.clone(),
      slot_player_id: (slot.id + 1) as _,
      w3gs_ack_q: W3GSAckQueue::new(),
      lag_duration_ms: 0,
      lag_start: None,
      lag_slot_ids: BTreeSet::new(),
      delay: None,
      action_rtt: None,
    }
  }

  pub fn player_name(&self) -> &str {
    self.player_name.as_str()
  }

  pub fn slot_player_id(&self) -> u8 {
    self.slot_player_id
  }

  pub fn ack_queue(&self) -> &W3GSAckQueue {
    &self.w3gs_ack_q
  }

  pub fn update_ack(&mut self, meta: W3GSMetadata) -> bool {
    if !self.w3gs_ack_q.ack_received(meta.sid()) {
      return false;
    }
    if let Some(ack_sid) = meta.ack_sid() {
      self.w3gs_ack_q.ack_sent(ack_sid);
    }
    true
  }

  pub fn register_sender(&mut self, tx: PlayerStreamHandle) {
    self.last_stream_id.replace(tx.stream_id());
    self.tx.replace(tx);
  }

  pub fn take_stream(&mut self) -> Option<PlayerStreamHandle> {
    self.tx.take()
  }

  pub fn close_stream(&mut self) -> Option<PlayerStreamHandle> {
    self.tx.take().map(|v| {
      v.close();
      v
    })
  }

  pub fn stream_id(&self) -> Option<u64> {
    self.tx.as_ref().map(|v| v.stream_id())
  }

  pub fn send_w3gs(&mut self, pkt: W3GSPacket) -> Result<(), PlayerSendError> {
    let meta = self.enqueue_w3gs(pkt.clone());
    self.send(Frame::from_w3gs(meta, pkt))
  }

  pub fn enqueue_w3gs(&mut self, pkt: W3GSPacket) -> W3GSMetadata {
    let sid = self.w3gs_ack_q.gen_next_send_sid();
    let ack_sid = self.w3gs_ack_q.take_ack_received();
    let meta = W3GSMetadata::new(pkt.type_id(), sid, ack_sid.clone());
    self.w3gs_ack_q.push_send(meta.clone(), pkt);
    meta
  }

  pub fn send_private_message(&mut self, msg: &str) {
    if self.stream_id().is_some() {
      let payload = ChatFromHost::private_to_self(self.slot_player_id, format!("[FLO] {}", msg));
      let pkt = match W3GSPacket::simple(payload) {
        Ok(pkt) => pkt,
        Err(err) => {
          tracing::warn!("encode broadcast message packet: {}", err);
          return;
        }
      };
      self.send_w3gs(pkt).ok();
    }
  }

  pub fn send(&mut self, frame: Frame) -> Result<(), PlayerSendError> {
    if let Some(tx) = self.tx.as_mut() {
      match tx.try_send(frame) {
        Ok(_) => Ok(()),
        Err(TrySendError::Closed(frame)) => Err(PlayerSendError::Closed(frame)),
        Err(TrySendError::Full(_)) => Err(PlayerSendError::ChannelFull),
      }
    } else {
      Err(PlayerSendError::NotConnected(frame))
    }
  }

  pub fn get_resend_frames(&self) -> Option<Vec<Frame>> {
    if self.w3gs_ack_q.pending_ack_len() > 0 {
      Some(
        self
          .w3gs_ack_q
          .pending_ack_queue()
          .iter()
          .map(|(meta, packet)| Frame::from_w3gs(meta.clone(), packet.clone()))
          .collect(),
      )
    } else {
      None
    }
  }

  pub fn start_lag(&mut self) -> u32 {
    if let Some(start) = self.lag_start {
      self.lag_duration_ms = self
        .lag_duration_ms
        .saturating_add((Instant::now() - start).as_millis() as u32);
    }
    self.lag_start.replace(Instant::now());
    self.lag_duration_ms
  }

  pub fn end_lag(&mut self) -> u32 {
    if let Some(start) = self.lag_start.take() {
      self.lag_duration_ms = self.lag_duration_ms.saturating_add(std::cmp::min(
        1000,
        (Instant::now() - start).as_millis() as u32,
      ));
    }
    self.lag_duration_ms
  }

  pub fn set_lag_slots<I: Iterator<Item = u8>>(&mut self, ids: I) {
    self.lag_slot_ids.clear();
    self.lag_slot_ids.extend(ids);
  }

  pub fn remove_lag_slot(&mut self, slot_id: u8) -> bool {
    self.lag_slot_ids.remove(&slot_id)
  }

  pub fn pristine(&self) -> bool {
    self.last_stream_id.is_none()
  }

  pub fn delay(&self) -> Option<&Duration> {
    self.delay.as_ref()
  }

  pub fn set_delay(&mut self, delay: Option<Duration>) -> Result<()> {
    self.delay = delay;
    if let Some(tx) = self.tx.as_ref() {
      tx.set_delay(self.delay.clone())?;
    }
    Ok(())
  }

  pub fn set_block(&mut self, delay: Duration) -> Result<()> {
    if let Some(tx) = self.tx.as_ref() {
      self.lag_duration_ms = delay.as_millis() as _;
      tx.set_block(delay)?;
    }
    Ok(())
  }

  pub fn set_action_rtt(&mut self, value: Duration) {
    self.action_rtt.replace(value);
  }

  pub fn action_rtt(&self) -> Option<Duration> {
    self.action_rtt.clone()
  }
}

pub enum PlayerSendError {
  NotConnected(Frame),
  Closed(Frame),
  ChannelFull,
  AckQueueFull,
}
