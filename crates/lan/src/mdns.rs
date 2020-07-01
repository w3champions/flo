use lazy_static::lazy_static;
use std::net::{Ipv4Addr, Ipv6Addr};
use trust_dns_client::op::{Message, MessageType, Query};
use trust_dns_client::proto::rr::rdata::{NULL, SRV, TXT};
use trust_dns_client::proto::xfer::SerialMessage;
use trust_dns_client::rr::{Name, RData, Record, RecordType};

use crate::game_info::GameInfo;
use flo_util::binary::*;

lazy_static! {
  pub static ref W3_SERVICE_NAME: Name =
    Name::from_ascii("_w3xp2730._sub._blizzard._udp.local.").unwrap();
  pub static ref BLIZZARD_SERVICE_NAME: Name = Name::from_ascii("_blizzard._udp.local.").unwrap();
  pub static ref GAME_NAME: Name = Name::from_ascii("flo._blizzard._udp.local.").unwrap();
  pub static ref DNS_SD_NAME: Name = Name::from_ascii("_services._dns-sd._udp.local.").unwrap();
  pub static ref FLO_NAME: Name = Name::from_ascii("F-2019.local.").unwrap();
}

pub fn get_ptr_response() -> Message {
  let mut msg = Message::new();
  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  let mut answer = Record::with(W3_SERVICE_NAME.clone(), RecordType::PTR, 0);
  answer.set_rdata(RData::PTR(GAME_NAME.clone())).set_ttl(0);
  msg.add_answer(answer);

  let mut answer = Record::with(BLIZZARD_SERVICE_NAME.clone(), RecordType::PTR, 0);
  answer.set_rdata(RData::PTR(GAME_NAME.clone())).set_ttl(0);
  msg.add_answer(answer);

  msg
}

pub fn get_response_message(query: &Message, port: u16, game_info: &mut GameInfo) -> Message {
  let mut msg = Message::new();

  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  let mut txt = false;
  let mut ptr = false;
  let mut srv = false;
  let mut info = false;

  for q in query.queries() {
    if q.name().eq(&W3_SERVICE_NAME)
      && (q.query_type() == RecordType::PTR || q.query_type() == RecordType::ANY)
    {
      ptr = true;
      txt = true;
      srv = true;
      info = true;
    }

    if q.name().eq(&GAME_NAME) {
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
      add_ptr_record(&mut msg);
    }

    if txt {
      msg.add_answer(get_txt_record());
    }

    if srv {
      msg.add_answer(get_srv_record(port));
      msg.add_additional(get_a_record());
      msg.add_additional(get_aaaa_record());
    }

    if info {
      msg.add_answer(get_gameinfo_record({
        game_info.message_id = game_info.message_id + 1;
        game_info.encode_to_bytes().unwrap()
      }));
    }
  }

  msg
}

pub fn get_broadcast_message(port: u16, game_info: &mut GameInfo) -> Message {
  let mut msg = Message::new();

  msg
    .set_message_type(MessageType::Response)
    .set_authoritative(true);

  msg.add_answer(get_txt_record());
  add_ptr_record(&mut msg);
  msg.add_answer(get_srv_record(port));
  msg.add_answer(get_gameinfo_record({
    game_info.message_id = game_info.message_id + 1;
    game_info.encode_to_bytes().unwrap()
  }));

  msg.add_additional(get_a_record());
  msg.add_additional(get_aaaa_record());

  msg
}

fn get_txt_record() -> Record {
  let mut record = Record::with(GAME_NAME.clone(), RecordType::TXT, 4500);
  record.set_mdns_cache_flush(true);
  record
}

fn add_ptr_record(m: &mut Message) {
  let mut record = Record::with(W3_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record
    .set_rdata(RData::PTR(GAME_NAME.clone()))
    .set_ttl(4500);
  m.add_answer(record);

  let mut record = Record::with(BLIZZARD_SERVICE_NAME.clone(), RecordType::PTR, 0);
  record.set_rdata(RData::PTR(GAME_NAME.clone())).set_ttl(0);
  m.add_answer(record);
}

fn get_gameinfo_record(game_info: Vec<u8>) -> Record {
  let mut record = Record::with(GAME_NAME.clone(), RecordType::Unknown(66), 4500);
  record
    .set_rdata(RData::NULL(NULL::with(game_info)))
    .set_ttl(4500)
    .set_mdns_cache_flush(true);
  record
}

fn get_srv_record(port: u16) -> Record {
  let mut record = Record::with(GAME_NAME.clone(), RecordType::SRV, 120);
  record
    .set_rdata(RData::SRV(SRV::new(0, 0, port, FLO_NAME.clone())))
    .set_mdns_cache_flush(true);
  record
}

fn get_a_record() -> Record {
  let mut record = Record::with(FLO_NAME.clone(), RecordType::A, 120);
  record
    .set_rdata(RData::A(Ipv4Addr::new(192, 168, 1, 6)))
    .set_mdns_cache_flush(true);
  record
}

fn get_aaaa_record() -> Record {
  let mut record = Record::with(FLO_NAME.clone(), RecordType::AAAA, 120);
  record
    .set_rdata(RData::AAAA(Ipv6Addr::LOCALHOST))
    .set_mdns_cache_flush(true);
  record
}

pub fn dump_message(m: &SerialMessage) {
  println!("Message from {}", m.addr());
  let m = m.to_message().unwrap();
  for q in m.queries() {
    println!("q: \t{}", q)
  }
  for a in m.answers() {
    println!("a: \t{:?}", (a.name(), a.dns_class()))
  }
  for a in m.additionals() {
    println!("additional: \t{:?}", (a.name(), a.dns_class()))
  }
}
