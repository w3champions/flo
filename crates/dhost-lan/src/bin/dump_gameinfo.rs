use dhost_lan::proto;
use futures::stream::StreamExt;
use prost::Message;
use trust_dns_proto::multicast::{MdnsClientStream, MdnsQueryType};
use trust_dns_proto::op::header::MessageType;
use trust_dns_proto::rr::record_data::RData;

#[tokio::main]
async fn main() {
  let (f, _) = MdnsClientStream::new_ipv4(MdnsQueryType::OneShotJoin, Some(255), None);
  let mut stream = f.await.unwrap();
  while let Some(res) = stream.next().await {
    match res {
      Ok(m) => {
        println!("from: {}", m.addr());
        println!("len: {}", m.bytes().len());
        match m.to_message() {
          Ok(msg) => match msg.message_type() {
            MessageType::Query => {}
            MessageType::Response => {
              println!("Response: {}", msg.answers().len());
              for r in msg.answers() {
                println!("- Name: {}", r.name());
                println!("- RR Type: {:?}", r.rr_type());
                println!("- Record Type: {:?}", r.record_type());
                println!("- Class: {}", r.dns_class());
                println!("- TTL: {}", r.ttl());
                println!("- Record Data: {:?}", r.rdata());
                if let RData::Unknown {
                  code: 66,
                  ref rdata,
                } = r.rdata()
                {
                  if let Some(bytes) = rdata.anything() {
                    let info: proto::GameInfo = Message::decode(bytes).expect("protobuf decode");
                    let data_entry = info
                      .entries
                      .iter()
                      .find(|i| i.key == "game_data")
                      .expect("find game_data entry");
                    let data = base64::decode(&data_entry.value).expect("game data decode");
                    let path = format!("dhost-lan/samples/gameinfo_{}.bin", info.name);
                    println!("- Info: {}", path);
                    std::fs::write(&path, bytes).unwrap();

                    let path = format!("dhost-lan/samples/gameinfo_{}.data.bin", info.name);
                    println!("- Data: {}", path);
                    std::fs::write(&path, data).unwrap();
                  }
                }
              }
            }
          },
          Err(e) => {
            eprintln!("deserialize error: {}", e);
          }
        }
      }
      Err(e) => {
        eprintln!("item error: {}", e);
      }
    }
  }
}
