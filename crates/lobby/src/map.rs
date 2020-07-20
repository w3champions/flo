use s2_grpc_utils::result::Error as ProtoError;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::Map")]
pub struct Map {
  pub sha1: MapSha1,
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
#[serde(transparent)]
pub struct MapSha1(pub [u8; 20]);

impl S2ProtoUnpack<Vec<u8>> for MapSha1 {
  fn unpack(value: Vec<u8>) -> Result<Self, ProtoError> {
    let mut bytes = [0_u8; 20];
    if value.len() >= 20 {
      bytes.clone_from_slice(&value[0..20]);
    } else {
      (&mut bytes[0..(value.len())]).clone_from_slice(&value[0..(value.len())]);
    }
    Ok(MapSha1(bytes))
  }
}

impl S2ProtoPack<Vec<u8>> for MapSha1 {
  fn pack(self) -> Result<Vec<u8>, ProtoError> {
    Ok(self.0.to_vec())
  }
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::MapPlayer")]
pub struct MapPlayer {
  pub name: String,
  pub r#type: u32,
  pub race: u32,
  pub flags: u32,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::MapForce")]
pub struct MapForce {
  pub name: String,
  pub flags: u32,
  pub player_set: u32,
}
