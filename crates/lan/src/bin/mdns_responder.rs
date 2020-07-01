use futures::{StreamExt, TryStreamExt};
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::multicast::MdnsQueryType;
use trust_dns_client::multicast::MdnsStream;
use trust_dns_client::proto::multicast::MDNS_IPV4;
use trust_dns_client::proto::xfer::SerialMessage;
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};
use trust_dns_client::serialize::binary::BinEncodable;

use flo_lan::mdns;
use flo_lan::mdns::get_broadcast_message;
use flo_lan::GameInfo;
use flo_util::binary::*;
use flo_w3gs::net::W3GSListener;

#[tokio::main]
async fn main() {
  let mut game_info =
    GameInfo::decode_bytes(&flo_util::sample_bytes!("lan", "mdns_gamedata.bin")).unwrap();

  let mut listener = W3GSListener::bind().await.unwrap();
  println!("W3GS listening on {}", listener.local_addr());
  let port = listener.port();

  game_info.data.port = port;
  game_info.data.settings.map_xoro = 0xFFFFFFFF;
  game_info.create_time = std::time::SystemTime::now();
  game_info.game_id = "2".to_owned();
  dbg!(&game_info);

  tokio::spawn(async move {
    loop {
      let stream = listener.accept().await;
      tokio::spawn(async move {
        use std::time::Duration;
        use tokio::time::delay_for;

        delay_for(Duration::from_millis(10 * 1000)).await;

        println!("dropping.");

        drop(stream)
      });
    }
  });

  let (stream, sender) = MdnsStream::new_ipv4(
    MdnsQueryType::OneShotJoin,
    Some(255),
    Some(Ipv4Addr::UNSPECIFIED),
  );

  sender.unbounded_send(SerialMessage::new(
    get_broadcast_message(port, &mut game_info)
      .to_vec()
      .unwrap(),
    *MDNS_IPV4,
  ));

  let mut stream = stream.await.unwrap();

  while let Some(msg) = stream.try_next().await.unwrap() {
    // mdns::dump_message(&msg);
    let msg = msg.to_message().unwrap();

    if !msg.queries().is_empty() {
      let res = mdns::get_response_message(&msg, port, &mut game_info);
      println!(
        "res: {:?}",
        res
          .answers()
          .iter()
          .map(|a| a.record_type())
          .collect::<Vec<_>>()
      );
      sender
        .unbounded_send(SerialMessage::new(res.to_vec().unwrap(), *MDNS_IPV4))
        .unwrap();
    }
  }
  println!("bye")
}
