mod mdns;

mod packets;

pub mod proto {
  include!(concat!(env!("OUT_DIR"), "/wc3.rs"));
}
