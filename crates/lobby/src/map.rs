use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Map {
  pub sha1: [u8; 20],
  pub checksum: u32,
  pub name: String,
  pub description: String,
  pub author: String,
  pub path: String,
  pub width: u32,
  pub height: u32,
  pub players: Vec<MapPlayer>,
  pub forces: Vec<MapForce>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapPlayer {
  pub name: String,
  #[serde(rename = "type")]
  pub type_: u32,
  pub race: u32,
  pub flags: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapForce {
  pub name: String,
  pub flags: u32,
  pub player_set: u32,
}
