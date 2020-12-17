use lazy_static::lazy_static;
use trust_dns_proto::rr::Name;

lazy_static! {
  pub static ref W3_SERVICE_NAME: Name =
    Name::from_ascii("_w3xp2730._sub._blizzard._udp.local").unwrap();
  pub static ref BLIZZARD_SERVICE_NAME: Name = Name::from_ascii("_blizzard._udp.local").unwrap();
}
