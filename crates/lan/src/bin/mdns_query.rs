use futures::{StreamExt, TryFutureExt};
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::multicast::MdnsQueryType;
use trust_dns_client::multicast::{MdnsClientStream, MdnsStream};
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};

#[tokio::main]
async fn main() {
  let (stream, sender) = MdnsClientStream::new_ipv4(MdnsQueryType::OneShotJoin, None, None);
  let (mut client, worker) =
    AsyncClient::with_timeout(stream, sender, std::time::Duration::from_secs(5), None)
      .await
      .unwrap();
  tokio::spawn(worker);
  let name = Name::from_ascii("_w3xp2730._sub._blizzard._udp.local.").unwrap();
  let msg = client
    .query(name, DNSClass::IN, RecordType::PTR)
    .await
    .unwrap();
  dbg!(msg);
}
