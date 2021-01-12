use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use s2_grpc_utils::result::Error as ProtoError;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use crate::schema::player;

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::player::Player")]
pub struct Player {
  pub id: i32,
  pub name: String,
  #[s2_grpc(proto_enum)]
  pub source: PlayerSource,
  pub source_id: String,
  pub source_state: Option<SourceState>,
  pub realm: Option<String>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type(
  flo_grpc::player::PlayerSource,
  flo_net::proto::flo_connect::PlayerSource
))]
pub enum PlayerSource {
  Test = 0,
  BNet = 1,
  Api = 2,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone, Queryable)]
#[s2_grpc(message_type(flo_grpc::player::PlayerRef, flo_net::proto::flo_connect::PlayerInfo))]
pub struct PlayerRef {
  pub id: i32,
  pub name: String,
  #[s2_grpc(proto_enum)]
  pub source: PlayerSource,
  pub realm: Option<String>,
}

pub(crate) type PlayerRefColumns = (
  player::dsl::id,
  player::dsl::name,
  player::dsl::source,
  player::dsl::realm,
);

impl PlayerRef {
  pub(crate) const COLUMNS: PlayerRefColumns = (
    player::dsl::id,
    player::dsl::name,
    player::dsl::source,
    player::dsl::realm,
  );
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceState {
  Invalid,
  BNet(BNetState),
}

impl S2ProtoUnpack<flo_grpc::player::PlayerSourceState> for SourceState {
  fn unpack(value: flo_grpc::player::PlayerSourceState) -> Result<Self, ProtoError> {
    use flo_grpc::player::player_source_state::SourceStateOneof;
    match value.source_state_oneof {
      Some(SourceStateOneof::Bnet(state)) => Ok(SourceState::BNet(S2ProtoUnpack::unpack(state)?)),
      None => Ok(SourceState::Invalid),
    }
  }
}

impl S2ProtoPack<flo_grpc::player::PlayerSourceState> for SourceState {
  fn pack(self) -> Result<flo_grpc::player::PlayerSourceState, ProtoError> {
    use flo_grpc::player::player_source_state::SourceStateOneof;
    use flo_grpc::player::PlayerSourceState;
    match self {
      SourceState::Invalid => Ok(Default::default()),
      SourceState::BNet(state) => Ok(PlayerSourceState {
        source_state_oneof: Some(SourceStateOneof::Bnet(state.pack()?)),
      }),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::player::BNetState")]
pub struct BNetState {
  pub account_id: u64,
  pub access_token: String,
  pub access_token_exp: u64,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type(
  flo_grpc::player::PlayerBanType,
  flo_net::proto::flo_node::PlayerBanType
))]
pub enum PlayerBanType {
  Chat = 0,
}
