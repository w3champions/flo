use bs_diesel_utils::BSDieselEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Player {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub source_id: String,
  pub realm: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum)]
#[repr(i32)]
pub enum PlayerSource {
  BNet = 0,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerRef {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub realm: Option<String>,
}
