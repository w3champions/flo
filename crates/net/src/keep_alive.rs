use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::delay_for;

use flo_util::uptime::uptime_ms;

use crate::error::Result;
use crate::packet::FloPacket;
use crate::proto::flo_common::{PacketPing, PacketPong};
use crate::stream::FloStream;

#[derive(Debug, Clone)]
pub struct KeepAlive {
  last_ping_ms: Arc<AtomicU32>,
  rtt_ms: Arc<AtomicU32>,
}

impl KeepAlive {
  pub fn new() -> Self {
    Self {
      last_ping_ms: Arc::new(AtomicU32::new(u32::MAX)),
      rtt_ms: Arc::new(AtomicU32::new(u32::MAX)),
    }
  }

  pub fn rtt_ms(&self) -> Option<u32> {
    let v = self.rtt_ms.load(Ordering::Relaxed);
    if v == u32::MAX {
      None
    } else {
      Some(v)
    }
  }

  pub async fn next(&self) -> KeepAliveItem {
    let last_ping_ms = self.last_ping_ms.load(Ordering::SeqCst);
    if last_ping_ms == u32::MAX {
      KeepAliveItem::Ping
    } else {
      let uptime = uptime_ms();
      if uptime - last_ping_ms > crate::constants::KEEP_ALIVE_TIMEOUT_MS {
        tracing::debug!(uptime, last_ping_ms, "timeout");
        KeepAliveItem::Timeout
      } else {
        let elapsed = uptime - last_ping_ms;
        let sleep = crate::constants::PING_INTERVAL_MS.saturating_sub(elapsed) as u64;
        delay_for(Duration::from_millis(sleep)).await;
        tracing::debug!(uptime, last_ping_ms, "ping");
        KeepAliveItem::Ping
      }
    }
  }

  /// Marks last ping time, send a `PingPacket`
  pub async fn send_ping(&self, stream: &mut FloStream) -> Result<()> {
    let ms = uptime_ms();
    self.last_ping_ms.store(ms, Ordering::SeqCst);
    stream.send(PacketPing { ms }).await?;
    Ok(())
  }

  /// Updates rtt, reset last ping time
  pub fn handle_pong(&self, pong: PacketPong) {
    self.last_ping_ms.store(u32::MAX, Ordering::SeqCst);
    let uptime = uptime_ms();
    let rtt = uptime.saturating_sub(pong.ms);
    tracing::debug!(uptime, rtt, "pong");
    self.rtt_ms.store(rtt, Ordering::Relaxed)
  }
}

pub enum KeepAliveItem {
  Ping,
  Timeout,
}
