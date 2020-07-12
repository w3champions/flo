use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Queryable)]
#[s2_grpc(message_type = "flo_grpc::player::Player")]
pub struct Player {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub source_id: String,
  pub source_state: Option<Value>,
  pub realm: Option<String>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type = "flo_grpc::player::PlayerSource")]
pub enum PlayerSource {
  BNet = 0,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::player::PlayerRef")]
pub struct PlayerRef {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub realm: Option<String>,
}

impl From<Player> for PlayerRef {
  fn from(p: Player) -> Self {
    PlayerRef {
      id: p.id,
      name: p.name,
      source: p.source,
      realm: p.realm,
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerId(i32);
