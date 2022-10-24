use s2_grpc_utils::S2ProtoEnum;

use dispatch::Dispatcher;
use flo_net::packet::*;
pub use sync::AckError;

use crate::error::*;
use crate::game::host::stream::{PlayerStream, PlayerStreamHandle};
use crate::game::{GameEventSender, NodeGameStatusSnapshot, PlayerSlot};
use crate::observer::ObserverPublisherHandle;
use flo_w3gs::constants::LeaveReason;

mod broadcast;
mod clock;
mod delay;
mod delay_equalizer;
mod dispatch;
mod player;
pub mod stream;
mod sync;

#[derive(Debug)]
pub struct GameHost {
  game_id: i32,
  dispatcher: Dispatcher,
}

#[derive(Debug)]
pub struct GameHostOptions {
  pub enabled_ping_equalizer: bool,
}

impl GameHost {
  pub fn new(
    game_id: i32,
    opts: GameHostOptions,
    slots: &[PlayerSlot],
    obs: ObserverPublisherHandle,
    event_sender: GameEventSender,
  ) -> Self {
    let dispatcher = Dispatcher::new(game_id, opts, slots, obs, event_sender);
    Self {
      game_id,
      dispatcher,
    }
  }

  pub fn start(&mut self) {
    self.dispatcher.start();
  }

  pub async fn register_player_stream(
    &mut self,
    mut stream: PlayerStream,
    snapshot: NodeGameStatusSnapshot,
  ) -> Result<PlayerStreamHandle> {
    let player_id = stream.player_id();
    stream
      .get_mut()
      .send_frames(vec![{
        let mut pkt = flo_net::proto::flo_node::PacketClientConnectAccept {
          version: Some(crate::version::FLO_NODE_VERSION.into()),
          game_id: self.game_id,
          player_id,
          ..Default::default()
        };
        pkt.set_game_status(snapshot.game_status.into_proto_enum());
        for (player_id, status) in snapshot.player_game_client_status_map {
          pkt.insert_player_game_client_status_map(player_id, status.into_proto_enum());
        }
        pkt
      }
      .encode_as_frame()?])
      .await?;

    self.dispatcher.register_player_stream(stream).await
  }

  pub async fn notify_player_shutdown(
    &mut self,
    player_id: i32,
    leave_reason: Option<LeaveReason>,
  ) -> Result<()> {
    self
      .dispatcher
      .notify_player_shutdown(player_id, leave_reason)
      .await
  }
}
