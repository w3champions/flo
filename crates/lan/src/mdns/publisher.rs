use parking_lot::RwLock;
use std::net::{Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tokio::time::delay_for;
use trust_dns_client::multicast::MdnsQueryType;
use trust_dns_client::multicast::MdnsStream;
use trust_dns_client::op::{Message, MessageType};
use trust_dns_client::proto::multicast::MDNS_IPV4;
use trust_dns_client::proto::rr::rdata::{NULL, SRV};
use trust_dns_client::proto::xfer::{BufStreamHandle, SerialMessage};
use trust_dns_client::rr::{Name, RData, Record, RecordType};

use tracing::{debug, error, instrument, span, warn, Level};
use tracing_futures::Instrument;

use crate::constants;
use crate::error::*;
use crate::game_info::GameInfo;
use trust_dns_client::serialize::binary::BinEncodable;
use flo_platform::net::IpInfo;

type GameInfoRef = Arc<RwLock<GameInfo>>;
type UpdateTx = mpsc::Sender<oneshot::Sender<()>>;

#[derive(Debug)]
pub struct MdnsPublisher {
  update_tx: UpdateTx,
  drop_tx: Option<oneshot::Sender<()>>,
  game_info: GameInfoRef,
}

impl MdnsPublisher {
  #[instrument(skip(game_info))]
  pub async fn start(game_info: GameInfo) -> Result<Self> {
    let name = game_info.name.to_string_lossy();
    let label = if name.bytes().len() > 31 {
      let name = name
        .char_indices()
        .filter_map(|(i, c)| {
          if i < 31 || (i == 31 && c.len_utf8() == 1) {
            Some(c)
          } else {
            None
          }
        })
        .collect::<String>();
      std::borrow::Cow::Owned(name)
    } else {
      name
    };
    let name = Name::from_labels(
      Some(label.as_bytes())
        .into_iter()
        .chain(constants::BLIZZARD_SERVICE_NAME.iter()),
    )?;
    let game_info = Arc::new(RwLock::new(game_info));
    let (update_tx, mut update_rx) = mpsc::channel::<oneshot::Sender<()>>(1);
    let (drop_tx, drop_rx) = oneshot::channel();
    let ip_info: IpInfo = flo_platform::net::get_ip_info()?;
    let hostname = hostname::get().map_err(Error::GetHostName)?;
    let hostname = if let Some(v) = hostname.to_str() {
      v.to_string()
    } else {
      warn!("non utf-8 host name");
      hostname.to_string_lossy().to_string()
    };
    let hostname = Name::from_labels(vec![hostname.as_bytes(), "local".as_bytes()])?;

    let (connect, sender) = MdnsStream::new_ipv4(
      MdnsQueryType::OneShotJoin,
      Some(255),
      Some(Ipv4Addr::UNSPECIFIED),
    );

    let mut stream = connect.await.map_err(Error::MdnsStreamBroken)?;

    let task_game_info = game_info.clone();
    let task = async move {
      debug!("started");

      let game_info = task_game_info;

      if let Err(e) = broadcast(&sender, &name, &hostname, &ip_info, game_info.clone()) {
        error!("broadcast initial update error: {}", e);
        return;
      }

      tokio::pin!(drop_rx);
      loop {
        tokio::select! {
          _ = &mut drop_rx => {
            debug!("signal received");
            tokio::spawn(async move {
              let stream = stream;
              broadcast_cancel(&sender, &name).ok();
              // hack: wait 2 second to flush the mdns udp stream
              tokio::time::timeout(Duration::from_secs(2), stream.collect::<Vec<_>>()).await.ok();
            });
            break;
          },
          update = update_rx.recv() => {
            if let Some(ack) = update {
              if let Err(e) = broadcast(
                &sender,
                &name,
                &hostname,
                &ip_info,
                game_info.clone()
              ) {
                error!("broadcast error: {}", e);
              }
              ack.send(()).ok();
            } else {
              debug!("update handle dropped");
              break;
            }
          },
          query = stream.try_next() => {
            match query {
              Ok(Some(query)) => {
                if let Err(e) = reply(
                  &sender,
                  query,
                  &name,
                  &hostname,
                  &ip_info,
                  game_info.clone(),
                ) {
                  error!("reply error: {}", e);
                }
              },
              Ok(None) => {
                debug!("mdns stream ended");
                break;
              }
              Err(e) => {
                error!("mdns stream broken: {}", e);
                break;
              }
            }
          }
        }
      }

      debug!("shutting down");
    }
    .instrument(span!(Level::DEBUG, "mdns_task"));

    tokio::spawn(task);

    Ok(Self {
      update_tx,
      drop_tx: Some(drop_tx),
      game_info,
    })
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

    tokio::select! {
      _ = delay_for(Duration::from_secs(1)) => Err(Error::MdnsUpdateGameInfo("timeout")),
      r = ack_rx => r.map_err(|_| Error::MdnsUpdateGameInfo("worker dead: recv")),
    }
  }
}

impl std::ops::Drop for MdnsPublisher {
  fn drop(&mut self) {
    if let Some(tx) = self.drop_tx.take() {
      debug!("dropping");
      tx.send(()).ok();
    }
  }
}

#[derive(Clone)]
pub struct GameInfoSender(Arc<mpsc::Sender<GameInfoRef>>);

fn reply(
  sender: &BufStreamHandle,
  query: SerialMessage,
  name: &Name,
  hostname: &Name,
  ip_info: &IpInfo,
  game_info: GameInfoRef,
) -> Result<()> {
  let mut game_info = game_info.write();
  let port = game_info.data.port;
  let sender_addr = query.addr();
  let query = query.to_message()?;

  let addr = if query.queries().iter().any(|q| q.mdns_unicast_response()) {
    sender_addr
  } else {
    *MDNS_IPV4
  };

  let mut msg = Message::new();

  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  let mut txt = false;
  let mut ptr = false;
  let mut srv = false;
  let mut info = false;

  for q in query.queries() {
    if q.name().eq(&constants::W3_SERVICE_NAME)
      && (q.query_type() == RecordType::PTR || q.query_type() == RecordType::ANY)
    {
      ptr = true;
      txt = true;
      srv = true;
      info = true;
    }

    if q.name().eq(name) {
      match q.query_type() {
        RecordType::ANY => {
          txt = true;
          srv = true;
          info = true;
        }
        RecordType::TXT => {
          txt = true;
        }
        RecordType::SRV => {
          srv = true;
        }
        RecordType::Unknown(66) => {
          info = true;
        }
        _ => {}
      }
    }
  }

  if ptr || txt || srv || info {
    if ptr {
      add_ptr_record(&mut msg, name);
    }

    if txt {
      msg.add_answer(get_txt_record(name));
    }

    if srv {
      msg.add_answer(get_srv_record(name, hostname, port));

      add_a_record(&mut msg, hostname, ip_info);
      add_aaaa_record(&mut msg, hostname, ip_info);
    }

    if info {
      msg.add_answer(get_gameinfo_record(name, {
        game_info.message_id = game_info.message_id + 1;
        game_info.encode_to_bytes()?
      }));
    }

    sender
      .unbounded_send(SerialMessage::new(msg.to_bytes()?, addr))
      .map_err(|e| Error::MdnsBroadcastError(e.to_string()))?;
  }
  Ok(())
}

fn broadcast(
  sender: &BufStreamHandle,
  name: &Name,
  hostname: &Name,
  ip_info: &IpInfo,
  game_info: GameInfoRef,
) -> Result<()> {
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

  sender
    .unbounded_send(SerialMessage::new(bytes.clone(), *MDNS_IPV4))
    .map_err(|e| Error::MdnsBroadcastError(e.to_string()))?;

  Ok(())
}

fn broadcast_cancel(sender: &BufStreamHandle, name: &Name) -> Result<()> {
  let mut msg = Message::new();

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

  sender
    .unbounded_send(SerialMessage::new(bytes.clone(), *MDNS_IPV4))
    .map_err(|e| Error::MdnsBroadcastError(e.to_string()))?;

  Ok(())
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
