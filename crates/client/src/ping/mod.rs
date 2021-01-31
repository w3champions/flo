use crate::error::Result;
use crate::ping::collect::{GetPingStats, PingCollectActor, PingReply, SetActive};
use flo_state::{async_trait, Actor, Addr, Container, Context, Handler, Message};
use flo_types::ping::PingStats;
use flo_util::binary::Ipv4Addr;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::delay_for;

mod collect;

pub struct PingActor {
  tx: mpsc::Sender<SendPing>,
  rx: Option<mpsc::Receiver<SendPing>>,
  map: BTreeMap<SocketAddr, Container<PingCollectActor>>,
}

impl PingActor {
  pub fn new() -> Self {
    let (tx, rx) = mpsc::channel(1);
    PingActor {
      tx,
      rx: Some(rx),
      map: Default::default(),
    }
  }

  async fn worker(addr: Addr<Self>, rx: &mut mpsc::Receiver<SendPing>) -> Result<(), PingError> {
    let mut socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    let mut buf = [0_u8; 4];
    loop {
      tokio::select! {
        Some(SendPing { to, data }) = rx.recv() => {
          socket.send_to(&data, to).await?;
        }
        Ok((size, from)) = socket.recv_from(&mut buf) => {
          if size == 4 {
            addr.send(RecvPong {
              from,
              data: buf
            }).await.map_err(|_| PingError::SenderGone)?;
          }
        }
        else => break,
      }
    }
    tracing::debug!("gone");
    Ok(())
  }
}

#[async_trait]
impl Actor for PingActor {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let mut rx = self.rx.take().unwrap();
    let addr = ctx.addr();
    ctx.spawn(async move {
      loop {
        if let Err(err) = Self::worker(addr.clone(), &mut rx).await {
          tracing::error!("ping worker error: {}", err)
        } else {
          tracing::error!("ping worker gone");
        }

        // wait 15s and recreate the worker
        delay_for(Duration::from_secs(15)).await;
      }
    })
  }
}

pub struct SendPing {
  to: SocketAddr,
  data: [u8; 4],
}

struct RecvPong {
  from: SocketAddr,
  data: [u8; 4],
}

impl Message for RecvPong {
  type Result = ();
}

#[async_trait]
impl Handler<RecvPong> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    RecvPong { from, data }: RecvPong,
  ) -> <RecvPong as Message>::Result {
    if let Some(v) = self.map.get_mut(&from) {
      v.notify(PingReply(data)).await.ok();
    }
  }
}

pub struct UpdateAddresses {
  pub addresses: Vec<SocketAddr>,
}

impl Message for UpdateAddresses {
  type Result = ();
}

#[async_trait]
impl Handler<UpdateAddresses> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateAddresses { addresses }: UpdateAddresses,
  ) -> <UpdateAddresses as Message>::Result {
    let add_keys: Vec<_> = addresses
      .iter()
      .filter(|addr| !self.map.contains_key(addr))
      .cloned()
      .collect();
    let remove_keys: Vec<_> = self
      .map
      .keys()
      .filter(|addr| !addresses.contains(addr))
      .cloned()
      .collect();
    for addr in add_keys {
      tracing::debug!("add addr: {}", addr);
      self
        .map
        .insert(addr, PingCollectActor::new(self.tx.clone(), addr).start());
    }

    for addr in remove_keys {
      tracing::debug!("remove addr: {}", addr);
      self.map.remove(&addr);
    }
  }
}

pub struct AddAddress {
  pub address: SocketAddr,
}

impl Message for AddAddress {
  type Result = ();
}

#[async_trait]
impl Handler<AddAddress> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    AddAddress { address }: AddAddress,
  ) -> <AddAddress as Message>::Result {
    tracing::debug!("add addr: {}", address);
    self.map.insert(
      address,
      PingCollectActor::new(self.tx.clone(), address).start(),
    );
  }
}

pub struct RemoveAddress {
  pub address: SocketAddr,
}

impl Message for RemoveAddress {
  type Result = ();
}

#[async_trait]
impl Handler<RemoveAddress> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    RemoveAddress { address }: RemoveAddress,
  ) -> <RemoveAddress as Message>::Result {
    tracing::debug!("remove addr: {}", address);
    self.map.remove(&address);
  }
}

pub struct GetPingMap;
impl Message for GetPingMap {
  type Result = BTreeMap<SocketAddr, PingStats>;
}

#[async_trait]
impl Handler<GetPingMap> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetPingMap,
  ) -> <GetPingMap as Message>::Result {
    use futures::stream::FuturesUnordered;

    let results: Vec<_> = self
      .map
      .values()
      .map(|v| v.send(GetPingStats))
      .collect::<FuturesUnordered<_>>()
      .collect()
      .await;

    results.into_iter().filter_map(|r| r.ok()).collect()
  }
}

pub struct GetAddressPing {
  pub address: SocketAddr,
}
impl Message for GetAddressPing {
  type Result = Result<Option<PingStats>>;
}

#[async_trait]
impl Handler<GetAddressPing> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetAddressPing { address }: GetAddressPing,
  ) -> <GetAddressPing as Message>::Result {
    if let Some(v) = self.map.get(&address) {
      Ok(Some(v.send(GetPingStats).await?.1))
    } else {
      Ok(None)
    }
  }
}

pub struct SetActiveAddress {
  pub address: Option<SocketAddr>,
}

impl Message for SetActiveAddress {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SetActiveAddress> for PingActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SetActiveAddress { address }: SetActiveAddress,
  ) -> <SetActiveAddress as Message>::Result {
    if let Some(address) = address {
      for (k, v) in &self.map {
        v.send(SetActive {
          active: *k == address,
        })
        .await?;
      }
    } else {
      for v in self.map.values() {
        v.send(SetActive { active: false }).await?;
      }
    }
    Ok(())
  }
}

#[derive(Error, Debug)]
pub enum PingError {
  #[error("sender timeout")]
  SenderTimeout,
  #[error("sender gone")]
  SenderGone,
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("time overflow")]
  TimeOverflow,
}

impl Message for PingError {
  type Result = ();
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingUpdate {
  pub ping_map: BTreeMap<i32, PingStats>,
}

#[tokio::test]
async fn test_ping() {
  use tokio::time::delay_for;

  flo_log_subscriber::init_env_override("DEBUG");

  let addresses: Vec<SocketAddr> = (&[
    "127.0.0.1",
    "139.162.28.191",
    "45.79.236.67",
    "172.104.228.116",
    "172.105.244.86",
    "173.255.233.174",
    "96.126.100.210",
  ])
    .into_iter()
    .map(|v| format!("{}:3552", v).parse::<SocketAddr>().unwrap())
    .collect();

  let actor = PingActor::new().start();
  actor.send(UpdateAddresses { addresses }).await.unwrap();

  for i in 0..10 {
    delay_for(std::time::Duration::from_secs(2)).await;
    tracing::debug!("getting #{}", i);
    let map = actor.send(GetPingMap).await.unwrap();
    tracing::debug!(
      "#{}: {:?}",
      i,
      map
        .iter()
        .map(|(k, v)| (k, v.avg.clone()))
        .collect::<BTreeMap<_, _>>()
    );
  }

  actor.shutdown().await.unwrap();
}
