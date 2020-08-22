use chrono::{DateTime, Utc};
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use crate::schema::node;

#[derive(Debug, Serialize, Deserialize, Queryable, Clone, S2ProtoPack)]
#[s2_grpc(message_type(flo_grpc::node::Node, flo_net::proto::flo_connect::Node))]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  #[s2_grpc(skip_pack)]
  pub secret: String,
  pub ip_addr: String,
  #[s2_grpc(skip_pack)]
  pub created_at: DateTime<Utc>,
  #[s2_grpc(skip_pack)]
  pub updated_at: DateTime<Utc>,
  pub country_id: String,
  #[s2_grpc(skip_pack)]
  pub disabled: bool,
}

pub type NodeRefColumns = (
  node::dsl::id,
  node::dsl::name,
  node::dsl::location,
  node::dsl::ip_addr,
  node::dsl::country_id,
);

#[derive(Debug, Serialize, Deserialize, Clone, S2ProtoPack, S2ProtoUnpack, Queryable)]
#[s2_grpc(message_type(flo_grpc::node::Node, flo_net::proto::flo_connect::Node))]
pub struct NodeRef {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub ip_addr: String,
  pub country_id: String,
}

impl NodeRef {
  pub const COLUMNS: NodeRefColumns = (
    node::dsl::id,
    node::dsl::name,
    node::dsl::location,
    node::dsl::ip_addr,
    node::dsl::country_id,
  );
}

impl From<Node> for NodeRef {
  fn from(node: Node) -> Self {
    NodeRef {
      id: node.id,
      name: node.name,
      location: node.location,
      ip_addr: node.ip_addr,
      country_id: node.country_id,
    }
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerToken {
  pub player_id: i32,
  pub bytes: [u8; 16],
}

impl PlayerToken {
  pub fn to_vec(&self) -> Vec<u8> {
    self.bytes.to_vec()
  }

  pub fn from_vec(player_id: i32, bytes: Vec<u8>) -> Option<Self> {
    if bytes.len() == 16 {
      Some(Self {
        player_id,
        bytes: {
          let mut value = [0_u8; 16];
          value.copy_from_slice(&bytes[..]);
          value
        },
      })
    } else {
      None
    }
  }

  pub fn as_slice(&self) -> &[u8] {
    &self.bytes
  }

  pub fn into_array(self) -> [u8; 16] {
    self.bytes
  }
}

#[derive(Debug, Queryable)]
pub struct NodeConnConfig {
  pub id: i32,
  pub addr: String,
  pub secret: String,
}
