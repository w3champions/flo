use pretty_hex::*;
use trust_dns_proto::op::header::{Header, MessageType};
use trust_dns_proto::op::query::Query;
use trust_dns_proto::rr::record_type::RecordType;
use trust_dns_proto::serialize::binary::{BinEncodable, BinEncoder};

fn main() {
  let mut buf = vec![];
  let mut e = BinEncoder::new(&mut buf);
  let header = {
    let mut header = Header::new();
    header.set_message_type(MessageType::Query);
    header.set_query_count(1);
    header
  };
  header.emit(&mut e).unwrap();

  let query = {
    let query = Query::query(
      "_w3xp2730._sub._blizzard._udp.local".parse().unwrap(),
      RecordType::PTR,
    );
    query
  };
  query.emit(&mut e).unwrap();

  println!("{:?}", buf.hex_dump());
}
