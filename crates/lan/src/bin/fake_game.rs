use futures::stream::TryStreamExt;
use std::ffi::CString;
use std::time::Duration;
use tokio::time::delay_for;
use tracing::debug;

use flo_lan::{GameInfo, MdnsPublisher};
#[tokio::main]
async fn main() {
  flo_log::init_debug("flo_lan");

  use flo_w3gs::net::W3GSListener;

  let mut listener = W3GSListener::bind().await.unwrap();
  println!("W3GS listening on {}", listener.local_addr());
  let port = listener.port();

  let mut game_info =
    GameInfo::decode_bytes(&flo_util::sample_bytes!("lan", "mdns_gamedata.bin")).unwrap();
  game_info.name = CString::new("测 试").unwrap();
  game_info.data.name = CString::new("测 试").unwrap();
  game_info.data.port = port;
  game_info.create_time = std::time::SystemTime::now();
  game_info.game_id = "2".to_owned();

  let _p = MdnsPublisher::start(game_info).await.unwrap();

  while let Some(stream) = listener.incoming().try_next().await.unwrap() {
    debug!("connected: {}", stream.addr());
    delay_for(Duration::from_secs(10)).await;
    debug!("dropped: {}", stream.addr());
  }
}
