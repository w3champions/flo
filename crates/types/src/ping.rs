use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, S2ProtoPack, S2ProtoUnpack, Serialize, Deserialize, Default)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PingStats, flo_grpc::player::PingStats))]
pub struct PingStats {
  pub min: Option<u32>,
  pub max: Option<u32>,
  pub avg: Option<u32>,
  pub current: Option<u32>,
  pub loss_rate: f32,
}
