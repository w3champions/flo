use super::{PingError, SendPing};
use crate::error::*;
use flo_net::time::StopWatch;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};
use flo_types::ping::PingStats;
use futures::future::{abortable, AbortHandle};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc::error::SendTimeoutError;
use tokio::sync::mpsc::Sender;
use tokio::time::delay_for;

const PACKETS: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(3);
const INTERVAL: Duration = Duration::from_secs(60);
const ACTIVE_PING_INTERVAL: Duration = Duration::from_secs(10);
const ERROR_DELAY: Duration = Duration::from_secs(60);

pub struct PingCollectActor {
  sock_addr_string: String,
  sock_addr: SocketAddr,
  batch_id: u8,
  results: [Option<u32>; PACKETS],
  current: Option<u32>,
  stop_watch: StopWatch,
  base_time: u32,
  sender: Sender<SendPing>,
  abort_timeout: Option<AbortHandle>,
  stats: PingStats,
  active: bool,
}

impl PingCollectActor {
  pub fn new(sender: Sender<SendPing>, sock_addr: SocketAddr) -> Self {
    Self {
      sender,
      sock_addr_string: format!("{}", sock_addr),
      sock_addr,
      batch_id: rand::random(),
      results: [None; PACKETS],
      current: None,
      stop_watch: StopWatch::new(),
      base_time: 0,
      abort_timeout: None,
      stats: PingStats::default(),
      active: false,
    }
  }

  fn interval(&self) -> Duration {
    if self.active {
      ACTIVE_PING_INTERVAL
    } else {
      INTERVAL
    }
  }

  async fn send_packets(
    addr: Addr<Self>,
    stop_watch: StopWatch,
    mut sender: Sender<SendPing>,
    sock_addr: SocketAddr,
    batch_id: u8,
  ) -> Result<(), PingError> {
    let mut buf = [0_u8; 4];
    let base_time = stop_watch.elapsed_ms();
    addr.send(SetBaseTime { base_time }).await.ok();
    for seq in 0..(PACKETS as u16) {
      buf[0] = seq as u8;
      buf[1] = batch_id;
      let t = stop_watch.elapsed_ms() - base_time;

      if t > u16::MAX as u32 {
        return Err(PingError::TimeOverflow);
      }
      (&mut buf[2..4]).copy_from_slice(&(t as u16).to_le_bytes());
      sender
        .send_timeout(
          SendPing {
            to: sock_addr,
            data: buf,
          },
          Duration::from_millis(50),
        )
        .await
        .map_err(|err| match err {
          SendTimeoutError::Timeout(_) => PingError::SenderTimeout,
          SendTimeoutError::Closed(_) => PingError::SenderGone,
        })?;
      if seq != (PACKETS as u16) - 1 {
        delay_for(Duration::from_millis(100)).await;
      }
    }
    Ok(())
  }

  fn collect_stats(&mut self) -> PingFinished {
    use std::convert::identity;
    let mut values: Vec<_> = self.results.iter().cloned().filter_map(identity).collect();
    values.sort();

    tracing::trace!("addr: {}, ping: {:?}", self.sock_addr, self.current);

    PingFinished {
      min: values.first().cloned(),
      max: values.last().cloned(),
      avg: if values.is_empty() {
        None
      } else {
        Some(values.iter().cloned().sum::<u32>() / values.len() as u32)
      },
      current: self.current.take(),
      loss_rate: (PACKETS - values.len()) as f32 / (PACKETS as f32),
    }
  }

  fn schedule_next(&mut self, ctx: &mut Context<Self>, delay: Duration) {
    self.abort_timeout.take().map(|v| v.abort());
    let addr = ctx.addr();
    ctx.spawn(async move {
      delay_for(delay).await;
      addr.send(PingStart).await.ok();
    });
  }

  fn start_ping(&mut self, ctx: &mut Context<Self>) {
    tracing::trace!(addr = self.sock_addr_string.as_str(), "start ping");
    self.results = [None; PACKETS];
    self.batch_id = self.batch_id.wrapping_add(1);
    self.current = None;

    let (timeout, abort) = abortable({
      let addr = ctx.addr();
      async move {
        delay_for(TIMEOUT).await;
        addr.notify(PingCollectTimeout).await.ok();
      }
    });
    self.abort_timeout = Some(abort);

    ctx.spawn({
      let addr = ctx.addr();
      let f = Self::send_packets(
        addr.clone(),
        self.stop_watch.clone(),
        self.sender.clone(),
        self.sock_addr.clone().into(),
        self.batch_id,
      );
      let address_string = self.sock_addr_string.clone();
      async move {
        if let Err(err) = f.await {
          tracing::error!(address = &address_string as &str, "send error: {}", err);
          addr.notify(err).await.ok();
        } else {
          timeout.await.ok();
        }
      }
    });
  }

  fn address_str(&self) -> &str {
    self.sock_addr_string.as_str()
  }
}

#[async_trait]
impl Actor for PingCollectActor {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    self.start_ping(ctx);
  }
}

struct PingStart;
impl Message for PingStart {
  type Result = ();
}

#[async_trait]
impl Handler<PingStart> for PingCollectActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    _: PingStart,
  ) -> <PingStart as Message>::Result {
    self.start_ping(ctx);
  }
}

struct SetBaseTime {
  base_time: u32,
}

impl Message for SetBaseTime {
  type Result = ();
}

#[async_trait]
impl Handler<SetBaseTime> for PingCollectActor {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    SetBaseTime { base_time }: SetBaseTime,
  ) -> <SetBaseTime as Message>::Result {
    self.base_time = base_time;
  }
}

struct PingCollectTimeout;
impl Message for PingCollectTimeout {
  type Result = ();
}

#[async_trait]
impl Handler<PingCollectTimeout> for PingCollectActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    _: PingCollectTimeout,
  ) -> <PingCollectTimeout as Message>::Result {
    tracing::debug!(address = self.address_str(), "ping timeout");
    self.stats = PingStats {
      current: None,
      loss_rate: 1.0,
      ..self.stats
    };
    self.schedule_next(ctx, ERROR_DELAY)
  }
}

pub struct PingReply(pub [u8; 4]);
impl Message for PingReply {
  type Result = ();
}

#[async_trait]
impl Handler<PingReply> for PingCollectActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    PingReply(bytes): PingReply,
  ) -> <PingReply as Message>::Result {
    let seq = bytes[0];
    let batch_id = bytes[1];

    if batch_id != self.batch_id {
      tracing::warn!(
        address = self.address_str(),
        "ping reply discarded: seq = {}, batch_id = {}, expected_batch_id = {}",
        seq,
        batch_id,
        self.batch_id
      );
      return;
    }

    let t = self
      .base_time
      .saturating_add(u16::from_le_bytes([bytes[2], bytes[3]]) as u32);

    let seq_max = (PACKETS as u8) - 1;

    if seq > seq_max {
      tracing::warn!(
        address = self.address_str(),
        "received out of range seq: {}",
        seq
      );
      return;
    }

    let now = self.stop_watch.elapsed_ms();

    if t <= now {
      let current = now - t;
      self.current = current.into();
      self.results[seq as usize] = Some(current);

      if self.results.iter().all(Option::is_some) {
        let finished = self.collect_stats();
        self.stats = PingStats {
          min: finished.min.or(self.stats.min),
          max: finished.max.or(self.stats.max),
          avg: finished.avg,
          current: finished.current,
          loss_rate: finished.loss_rate,
        };
        self.schedule_next(ctx, self.interval());
      }
    } else {
      tracing::debug!(address = self.address_str(), "invalid time value: {}", t);
    }
  }
}

#[derive(Debug)]
pub struct PingFinished {
  pub min: Option<u32>,
  pub max: Option<u32>,
  pub avg: Option<u32>,
  pub current: Option<u32>,
  pub loss_rate: f32,
}

impl Message for PingFinished {
  type Result = ();
}

#[async_trait]
impl Handler<PingError> for PingCollectActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    message: PingError,
  ) -> <PingError as Message>::Result {
    tracing::debug!(address = self.address_str(), "ping error: {}", message);
    self.stats.loss_rate = 1.0;
    self.stats.current = None;
    self.schedule_next(ctx, ERROR_DELAY);
  }
}

pub struct GetPingStats;

impl Message for GetPingStats {
  type Result = (SocketAddr, PingStats);
}

#[async_trait]
impl Handler<GetPingStats> for PingCollectActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetPingStats,
  ) -> <GetPingStats as Message>::Result {
    (self.sock_addr, self.stats.clone())
  }
}

pub struct SetActive {
  pub active: bool,
}

impl Message for SetActive {
  type Result = ();
}

#[async_trait]
impl Handler<SetActive> for PingCollectActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SetActive { active }: SetActive,
  ) -> <SetActive as Message>::Result {
    self.active = active;
  }
}

#[tokio::test]
async fn test_ping_collect() {
  use std::net::Ipv4Addr;
  use std::sync::Arc;
  use tokio::net::UdpSocket;
  use tokio::sync::mpsc;
  use tokio::sync::Notify;

  flo_log_subscriber::init_env_override("DEBUG");
  let notify = Arc::new(Notify::new());

  let mut socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
  let (tx, mut rx) = mpsc::channel(1);
  let sock_addr = SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), 3552));

  let actor = {
    let mut a = PingCollectActor::new(tx, sock_addr);
    a.active = true;
    a
  }
  .start();
  let addr = actor.addr();

  tokio::spawn({
    let notify = notify.clone();
    async move {
      let mut buf = [0_u8; 4];
      let mut n = 0_usize;
      loop {
        tokio::select! {
          next = rx.recv() => {
            if n > 10 {
              notify.notify();
            }
            let data = next.unwrap();
            socket.send_to(&data.data, sock_addr).await.unwrap();
            n += 1;
          }
          next = socket.recv(&mut buf) => {
            if next.unwrap() == 4 {
              addr.send(PingReply(buf)).await.unwrap();
            }
          }
        }
      }
    }
  });

  notify.notified().await;

  tracing::debug!("shutting down");

  let stats = actor.shutdown().await.unwrap().stats;
  tracing::debug!("{:?}", stats);

  assert_eq!(stats.loss_rate, 0.0);
  assert!(stats.max.unwrap() <= 1);
}
