use crate::error::*;
use crate::game::db::UpdateSlotSettings;
use crate::game::state::GameActor;
use crate::game::{Slot, SlotSettings};
use diesel::prelude::*;
use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Context, Handler, Message};
use s2_grpc_utils::S2ProtoPack;

pub struct UpdateSlot {
  pub player_id: i32,
  pub slot_index: i32,
  pub settings: SlotSettings,
}

impl Message for UpdateSlot {
  type Result = Result<Vec<Slot>>;
}

#[async_trait]
impl Handler<UpdateSlot> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateSlot {
      player_id,
      slot_index,
      settings,
    }: UpdateSlot,
  ) -> Result<Vec<Slot>> {
    let game_id = self.game_id;

    let UpdateSlotSettings {
      slots,
      updated_indexes,
    } = self
      .db
      .exec(move |conn| {
        conn.transaction(|| {
          let info = crate::game::db::get_slot_owner_info(conn, game_id, slot_index)?;
          if !info.is_slot_owner(player_id) {
            return Err(Error::GameSlotUpdateDenied);
          }
          crate::game::db::update_slot_settings(conn, game_id, slot_index, settings)
        })
      })
      .await?;

    let mut frames_slot_update = Vec::with_capacity(updated_indexes.len());

    for index in updated_indexes {
      let slot = &slots[index as usize];
      let settings: proto::flo_connect::SlotSettings = slot.settings.clone().pack()?;
      let frame = proto::flo_connect::PacketGameSlotUpdate {
        game_id,
        slot_index: index,
        slot_settings: settings.into(),
        player: slot.player.clone().map(|p| p.pack()).transpose()?,
      }
      .encode_as_frame()?;
      frames_slot_update.push(frame);
    }

    let players = slots
      .iter()
      .filter_map(|s| s.player.as_ref().map(|p| p.id))
      .collect();
    self
      .player_packet_sender
      .broadcast(players, frames_slot_update)
      .await?;

    Ok(slots)
  }
}
