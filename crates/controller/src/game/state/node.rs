use crate::error::*;
use crate::game::state::{GameActor};


use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Context, Handler, Message};


pub struct SelectNode {
  pub node_id: Option<i32>,
  pub player_id: i32,
}

impl Message for SelectNode {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SelectNode> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SelectNode { node_id, player_id }: SelectNode,
  ) -> Result<()> {
    let game_id = self.game_id;

    if self.started() {
      return Err(Error::GameStarted);
    }

    self
      .db
      .exec(move |conn| crate::game::db::select_node(conn, game_id, player_id, node_id))
      .await?;

    self.selected_node_id = node_id;

    let frame = proto::flo_connect::PacketGameSelectNode { game_id, node_id }.encode_as_frame()?;
    self
      .player_packet_sender
      .broadcast(self.players.clone(), frame)
      .await?;

    Ok(())
  }
}
