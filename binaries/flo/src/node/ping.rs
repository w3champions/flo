use futures::StreamExt;
use parking_lot::RwLock;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch::{channel, Sender};
use tokio::sync::{mpsc, Notify, Semaphore};
use tokio::time::{delay_for, timeout};
use tracing_futures::Instrument;

use flo_constants::NODE_ECHO_PORT;
use flo_net::time::StopWatch;

use crate::error::*;
use crate::node::PingUpdate;

const TIMEOUT: Duration = Duration::from_secs(2);
const PACKETS: usize = 1;

#[derive(Debug)]
pub struct Pinger {
  interval_sender: Sender<PingIntervalUpdate>,
  current_interval: RwLock<Duration>,
  shutdown: Arc<Notify>,
  current_ping: Arc<RwLock<Option<u32>>>,
}

impl Pinger {
  pub fn new(
    limiter: Arc<Semaphore>,
    interval: Duration,
    mut update_sender: mpsc::Sender<PingUpdate>,
    node_id: i32,
    ip: Ipv4Addr,
    port: u16,
  ) -> Self {
    let shutdown = Arc::new(Notify::new());
    let ip_str: &str = &format!("{}", ip);
    let (interval_sender, mut interval_receiver) = channel(PingIntervalUpdate {
      value: interval,
      immediate: true,
    });
    let current_ping = Arc::new(RwLock::new(None));

    tokio::spawn(
      {
        let shutdown = shutdown.clone();
        let current_ping = current_ping.clone();
        async move {
          let mut sleep: Option<Duration> = None;
          interval_receiver.next().await; // consume the initial value
          loop {
            if let Some(duration) = sleep.take() {
              tokio::select! {
                _ = shutdown.notified() => {
                  tracing::debug!("exiting: shutdown");
                  break;
                }
                _ = delay_for(duration) => {},
                update = interval_receiver.next() => {
                  match update {
                    Some(update) => {
                      tracing::debug!("interval update: value = {:?}, immediate = {}", update.value, update.immediate);
                      if !update.immediate {
                        sleep = Some(update.value);
                        continue;
                      }
                    },
                    None => {
                      tracing::debug!("exiting: interval sender dropped");
                      break;
                    }
                  }
                }
              }
            }

            tokio::select! {
              _ = shutdown.notified() => {
                tracing::debug!("exiting: shutdown");
                break;
              }
              rtt = Self::ping_limited(limiter.clone(), ip.clone(), port) => {
                match rtt {
                  Ok(rtt) => {
                    *current_ping.write() = Some(rtt);
                    if let Err(_) = update_sender.send(PingUpdate {
                      node_id,
                      ping: Some(rtt)
                    }).await {
                      tracing::debug!("exiting: update receiver dropped");
                      break;
                    }
                  },
                  Err(_) => {
                    current_ping.write().take();
                    if let Err(_) = update_sender.send(PingUpdate {
                      node_id,
                      ping: None
                    }).await {
                      tracing::debug!("exiting: update receiver dropped");
                      break;
                    }
                  }
                }
              }
            }

            sleep = Some(interval_receiver.borrow().value.clone());
          }
        }
      }
      .instrument(tracing::debug_span!("worker", ip = ip_str, port = port)),
    );

    Self {
      interval_sender,
      current_interval: RwLock::new(interval),
      shutdown,
      current_ping,
    }
  }

  pub fn set_interval(&self, value: Duration, immediate: bool) -> Result<()> {
    *self.current_interval.write() = value;
    self
      .interval_sender
      .broadcast(PingIntervalUpdate { value, immediate })
      .map_err(|_| Error::SetPingIntervalFailed)?;
    Ok(())
  }

  pub fn current_interval(&self) -> Duration {
    self.current_interval.read().clone()
  }

  pub fn current_ping(&self) -> Option<u32> {
    self.current_ping.read().clone()
  }

  async fn ping_limited(limiter: Arc<Semaphore>, ip: Ipv4Addr, port: u16) -> Result<u32> {
    let _permit = limiter.acquire().await;
    let rtt = ping(ip, Some(port)).await?;
    Ok(rtt)
  }
}

impl Drop for Pinger {
  fn drop(&mut self) {
    self.shutdown.notify();
  }
}

#[derive(Debug, Clone)]
struct PingIntervalUpdate {
  value: Duration,
  immediate: bool,
}

async fn ping(ip: Ipv4Addr, port: Option<u16>) -> Result<u32> {
  let port = port.unwrap_or(NODE_ECHO_PORT);

  let mut socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;

  timeout(TIMEOUT, socket.connect((ip, port)))
    .await
    .map_err(|_| Error::PingNodeTimeout)??;

  let sw = StopWatch::new();

  let mut buf = [0_u8; 4];

  for _ in 0..PACKETS {
    let t = sw.elapsed_ms();
    buf.copy_from_slice(&t.to_le_bytes());
    timeout(TIMEOUT, socket.send(&buf))
      .await
      .map_err(|_| Error::PingNodeTimeout)??;
  }

  let mut total_rtt: u32 = 0;

  for _ in 0..PACKETS {
    let size = timeout(TIMEOUT, socket.recv(&mut buf))
      .await
      .map_err(|_| Error::PingNodeTimeout)??;

    if size != 4 {
      return Err(Error::InvalidPingNodeReply);
    }

    let t = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let rtt = sw.elapsed_ms().saturating_sub(t);

    total_rtt = total_rtt.saturating_add(rtt);
  }

  Ok(total_rtt / PACKETS as u32)
}

#[tokio::test]
async fn test_ping() {
  let ms = ping(Ipv4Addr::new(127, 0, 0, 1), None).await.unwrap();
  dbg!(ms);
}
