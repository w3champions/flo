use flo_net::proto::flo_connect::{PacketPlayerSessionUpdate, PlayerStatus};

pub(super) fn get_session_update_packet(game_id: Option<i32>) -> PacketPlayerSessionUpdate {
  PacketPlayerSessionUpdate {
    status: if game_id.is_some() {
      PlayerStatus::InGame.into()
    } else {
      PlayerStatus::Idle.into()
    },
    game_id,
  }
}
