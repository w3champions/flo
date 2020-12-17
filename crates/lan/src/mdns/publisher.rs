use crate::constants;
use crate::error::*;
use crate::game_info::GameInfo;
use flo_platform::net::IpInfo;
use futures::future::TryFutureExt;
use parking_lot::RwLock;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tracing_futures::Instrument;
use trust_dns_proto::multicast::MDNS_IPV4;
use trust_dns_proto::op::{Message, MessageType};
use trust_dns_proto::rr::rdata::{NULL, SRV};
use trust_dns_proto::rr::{Name, RData, Record, RecordType};

type GameInfoRef = Arc<RwLock<GameInfo>>;
type UpdateTx = mpsc::Sender<oneshot::Sender<()>>;

#[derive(Debug)]
pub struct MdnsPublisher {
  update_tx: UpdateTx,
  game_info: GameInfoRef,
}

impl MdnsPublisher {
  pub async fn start(game_info: GameInfo) -> Result<Self> {
    let game_name = game_info.name.to_string_lossy().to_string();
    let game_info = Arc::new(RwLock::new(game_info));
    let (update_tx, update_rx) = mpsc::channel::<oneshot::Sender<()>>(1);
    let ip_info: IpInfo = flo_platform::net::get_ip_info()?;
    let hostname = hostname::get().map_err(Error::GetHostName)?;
    let hostname = if let Some(v) = hostname.to_str() {
      v.to_string()
    } else {
      tracing::warn!("non utf-8 host name");
      hostname.to_string_lossy().to_string()
    };
    let hostname = Name::from_labels(vec![hostname.as_bytes(), "local".as_bytes()])?;

    tokio::spawn(
      Self::worker(game_info.clone(), game_name, ip_info, hostname, update_rx)
        .map_err(|err| {
          tracing::error!("worker exited with error: {}", err);
        })
        .instrument(tracing::debug_span!("worker")),
    );

    Ok(Self {
      update_tx,
      game_info,
    })
  }

  async fn worker(
    game_info: GameInfoRef,
    game_name: String,
    ip_info: IpInfo,
    hostname: Name,
    mut update_rx: mpsc::Receiver<oneshot::Sender<()>>,
  ) -> Result<()> {
    let label = if game_name.bytes().len() > 31 {
      let name = game_name
        .char_indices()
        .filter_map(|(i, c)| {
          if i < 31 || (i == 31 && c.len_utf8() == 1) {
            Some(c)
          } else {
            None
          }
        })
        .collect::<String>();
      name
    } else {
      game_name
    };
    let name = Name::from_labels(
      Some(label.as_bytes())
        .into_iter()
        .chain(constants::BLIZZARD_SERVICE_NAME.iter()),
    )?;

    tracing::debug!("service name = {:?}", name);
    tracing::debug!("hostname = {:?}", hostname);

    let (mut socket, addr) = bind().await?;
    tracing::debug!("bind to addr: {}", addr);

    let publish_message = build_publish_message(&name, &hostname, &ip_info, game_info.clone())?;
    socket
      .send_to(&publish_message, *MDNS_IPV4)
      .await
      .map_err(Error::MdnsBroadcast)?;

    let mut recv_buf = [0; 1500];

    loop {
      tokio::select! {
        update = update_rx.recv() => {
          tracing::debug!("update");
          if let Some(ack) = update {
            let publish_message = build_publish_message(&name, &hostname, &ip_info, game_info.clone())?;
            socket.send_to(&publish_message, *MDNS_IPV4).await.map_err(Error::MdnsBroadcast)?;
            ack.send(()).ok();
          } else {
            tracing::debug!("update handle dropped");
            break;
          }
        },
        recv = socket.recv_from(&mut recv_buf) => {
          match recv {
            Ok((len, addr)) => {
              if len == 0 {
                continue;
              }
              tracing::debug!("query from: {}", addr);
            }
            Err(e) => {
              tracing::error!("mdns socket broken: {}", e);
              break;
            }
          }
        }
      }
    }

    tracing::debug!("shutting down");
    socket
      .send_to(&build_cancel_message(&name)?, *MDNS_IPV4)
      .await
      .map_err(Error::MdnsBroadcast)?;
    tracing::debug!("exiting");
    Ok(())
  }

  pub async fn update<F>(&mut self, f: F) -> Result<()>
  where
    F: FnOnce(&mut GameInfo),
  {
    {
      let mut lock = self.game_info.write();
      f(&mut lock)
    }
    self.refresh().await?;
    Ok(())
  }

  pub async fn refresh(&mut self) -> Result<()> {
    let (ack_tx, ack_rx) = oneshot::channel();
    self
      .update_tx
      .send(ack_tx)
      .await
      .map_err(|_| Error::MdnsUpdateGameInfo("worker dead: send"))?;

    tokio::time::timeout(Duration::from_secs(1), ack_rx)
      .await
      .map_err(|_| Error::MdnsUpdateGameInfo("timeout"))?
      .map_err(|_| Error::MdnsUpdateGameInfo("worker dead: recv"))
  }
}

async fn bind() -> Result<(UdpSocket, SocketAddr)> {
  let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
    .await
    .map_err(Error::MdnsStreamBind)?;
  socket.set_broadcast(true).map_err(Error::MdnsStreamBind)?;
  let addr = socket.local_addr().map_err(Error::MdnsStreamBind)?;
  Ok((socket, addr))
}

#[derive(Clone)]
pub struct GameInfoSender(Arc<mpsc::Sender<GameInfoRef>>);

fn build_publish_message(
  name: &Name,
  hostname: &Name,
  ip_info: &IpInfo,
  game_info: GameInfoRef,
) -> Result<Vec<u8>> {
  let mut game_info = game_info.write();
  let port = game_info.data.port;
  let mut msg = Message::new();

  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  msg.add_answer(get_txt_record(name));
  add_ptr_record(&mut msg, name);
  msg.add_answer(get_srv_record(name, hostname, port));
  msg.add_answer(get_gameinfo_record(name, {
    game_info.message_id = game_info.message_id + 1;
    game_info.encode_to_bytes()?
  }));

  add_a_record(&mut msg, hostname, ip_info);
  add_aaaa_record(&mut msg, hostname, ip_info);

  let bytes = msg.to_vec()?;

  Ok(bytes)
}

fn build_cancel_message(name: &Name) -> Result<Vec<u8>> {
  let mut msg = Message::new();

  tracing::debug!("cancel name: {:?}", name);

  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  let mut record = Record::with(constants::W3_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record.set_rdata(RData::PTR(name.clone())).set_ttl(0);
  msg.add_answer(record);

  let mut record = Record::with(constants::BLIZZARD_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record.set_rdata(RData::PTR(name.clone())).set_ttl(0);
  msg.add_answer(record);

  let bytes = msg.to_vec()?;

  Ok(bytes)
}

fn get_txt_record(name: &Name) -> Record {
  let mut record = Record::with(name.clone(), RecordType::TXT, 4500);
  record.set_mdns_cache_flush(true);
  record
}

fn add_ptr_record(m: &mut Message, name: &Name) {
  let mut record = Record::with(constants::W3_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record.set_rdata(RData::PTR(name.clone())).set_ttl(4500);
  m.add_answer(record);

  let mut record = Record::with(constants::BLIZZARD_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record.set_rdata(RData::PTR(name.clone())).set_ttl(0);
  m.add_answer(record);
}

fn get_gameinfo_record(name: &Name, game_info: Vec<u8>) -> Record {
  let mut record = Record::with(name.clone(), RecordType::Unknown(66), 4500);
  record
    .set_rdata(RData::NULL(NULL::with(game_info)))
    .set_ttl(4500)
    .set_mdns_cache_flush(true);
  record
}

fn get_srv_record(name: &Name, hostname: &Name, port: u16) -> Record {
  let mut record = Record::with(name.clone(), RecordType::SRV, 120);
  record
    .set_rdata(RData::SRV(SRV::new(0, 0, port, hostname.clone())))
    .set_mdns_cache_flush(true);
  record
}

fn add_a_record(msg: &mut Message, hostname: &Name, ipinfo: &IpInfo) {
  for ip in &ipinfo.ips_v4 {
    let mut record = Record::with(hostname.clone(), RecordType::A, 120);
    record
      .set_rdata(RData::A(ip.clone()))
      .set_mdns_cache_flush(true);
    msg.add_additional(record);
  }
}

fn add_aaaa_record(msg: &mut Message, hostname: &Name, ipinfo: &IpInfo) {
  for ip in &ipinfo.ips_v6 {
    let mut record = Record::with(hostname.clone(), RecordType::AAAA, 120);
    record
      .set_rdata(RData::AAAA(ip.clone()))
      .set_mdns_cache_flush(true);
    msg.add_additional(record);
  }
}
