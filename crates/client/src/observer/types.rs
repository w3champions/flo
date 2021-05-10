use flo_types::game::Slot;

#[derive(Debug)]
pub struct ObserverGameInfo {
  pub map_path: String,
  pub map_sha1: [u8; 20],
  pub slots: Vec<Slot>,
  pub random_seed: i32,
}
