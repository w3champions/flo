use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_node::NodeGameStatus")]
pub enum NodeGameStatus {
  Created = 0,
  Waiting = 1,
  Loading = 2,
  Running = 3,
  Ended = 4,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::SlotClientStatus")]
pub enum SlotClientStatus {
  Pending = 0,
  Connected = 1,
  Joined = 2,
  Loading = 3,
  Loaded = 4,
  Disconnected = 5,
  Left = 6,
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_net::proto::flo_node::PacketClientConnectAccept")]
pub struct NodeGameStatusSnapshot {
  pub game_id: i32,
  pub game_status: NodeGameStatus,
  pub player_game_client_status_map: HashMap<i32, SlotClientStatus>,
}
