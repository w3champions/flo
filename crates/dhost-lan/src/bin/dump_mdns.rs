use futures::stream::StreamExt;
use trust_dns_proto::multicast::{MdnsClientStream, MdnsQueryType};
use trust_dns_proto::op::header::MessageType;

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
            MessageType::Query => {
              println!("Query: {}", msg.queries().len());
              for q in msg.queries() {
                println!("- Name: {}", q.name());
                println!("- Type: {}", q.query_type());
                println!("- Class: {}", q.query_class());
              }
            }
            MessageType::Response => {
              println!("Response: {}", msg.answers().len());
              for r in msg.answers() {
                println!("- Name: {}", r.name());
                println!("- RR Type: {:?}", r.rr_type());
                println!("- Record Type: {:?}", r.record_type());
                println!("- Class: {}", r.dns_class());
                println!("- TTL: {}", r.ttl());
                println!("- Record Data: {:?}", r.rdata());
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
