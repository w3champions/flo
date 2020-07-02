use lazy_static::lazy_static;
use trust_dns_client::rr::Name;

lazy_static! {
  pub static ref W3_SERVICE_NAME: Name =
    Name::from_ascii("_w3xp2730._sub._blizzard._udp.local").unwrap();
  pub static ref BLIZZARD_SERVICE_NAME: Name = Name::from_ascii("_blizzard._udp.local").unwrap();
  pub static ref GAME_NAME: Name = Name::from_ascii("flo._blizzard._udp.local").unwrap();
  pub static ref DNS_SD_NAME: Name = Name::from_ascii("_services._dns-sd._udp.local").unwrap();
}
