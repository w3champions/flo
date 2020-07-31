use chrono::{DateTime, Utc};
use s2_grpc_utils::result::Error as ProtoError;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use flo_net::proto::flo_connect as packet;

#[derive(Debug, Serialize, Deserialize, Queryable)]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub secret: String,
  pub ip_addr: String,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum NodeRef {
  Public {
    id: i32,
    name: String,
    location: String,
    ip_addr: String,
  },
  Private {
    name: String,
    location: String,
    ip_addr: String,
    secret: String,
  },
}

impl S2ProtoUnpack<flo_grpc::game::SelectedNode> for NodeRef {
  fn unpack(value: flo_grpc::game::SelectedNode) -> Result<Self, ProtoError> {
    use flo_grpc::game::SelectedNodeType;
    match SelectedNodeType::from_i32(value.r#type) {
      Some(SelectedNodeType::Public) => Ok(Self::Public {
        id: value
          .id
          .ok_or_else(|| ProtoError::FieldValueNotPresent { field_name: "id" })?,
        name: value.name,
        location: value.location,
        ip_addr: value.ip_addr,
      }),
      Some(SelectedNodeType::Private) => Ok(Self::Private {
        name: value.name,
        location: value.location,
        ip_addr: value.ip_addr,
        secret: value
          .secret
          .ok_or_else(|| ProtoError::FieldValueNotPresent {
            field_name: "secret",
          })?,
      }),
      None => Err(ProtoError::EnumDiscriminantNotFound {
        discriminant: value.r#type,
        enum_name: "SelectedNode",
      }),
    }
  }
}

impl S2ProtoPack<flo_grpc::game::SelectedNode> for NodeRef {
  fn pack(self) -> Result<flo_grpc::game::SelectedNode, ProtoError> {
    use flo_grpc::game::{SelectedNode, SelectedNodeType};
    match self {
      Self::Public {
        id,
        name,
        location,
        ip_addr,
      } => Ok(SelectedNode {
        r#type: SelectedNodeType::Public.into(),
        id: Some(id),
        name,
        location,
        ip_addr,
        secret: None,
      }),
      Self::Private {
        name,
        location,
        ip_addr,
        secret,
      } => Ok(SelectedNode {
        r#type: SelectedNodeType::Private.into(),
        id: None,
        name,
        location,
        ip_addr,
        secret: Some(secret),
      }),
    }
  }
}

impl From<Node> for NodeRef {
  fn from(node: Node) -> Self {
    NodeRef::Public {
      id: node.id,
      name: node.name,
      location: node.location,
      ip_addr: node.ip_addr,
    }
  }
}

impl NodeRef {
  pub fn into_packet(self) -> packet::SelectedNode {
    match self {
      Self::Public {
        id,
        name,
        location,
        ip_addr,
      } => packet::SelectedNode {
        r#type: packet::SelectedNodeType::Public.into(),
        id: Some(id),
        name,
        location,
        ip_addr,
        secret: None,
      },
      Self::Private {
        name,
        location,
        ip_addr,
        secret,
      } => packet::SelectedNode {
        r#type: packet::SelectedNodeType::Private.into(),
        id: None,
        name,
        location,
        ip_addr,
        secret: Some(secret),
      },
    }
  }
}
