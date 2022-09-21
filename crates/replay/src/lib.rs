pub mod error;
use bytes::Bytes;
use error::{Error, Result};

use flo_net::w3gs::W3GSPacketTypeId;
use flo_observer::record::GameRecordData;
use flo_observer_fs::GameDataArchiveReader;
use flo_types::game::SlotStatus;
use flo_w3gs::leave::LeaveReason;
use flo_w3gs::player::{PlayerProfileMessage, PlayerSkinsMessage, PlayerUnknown5Message};
use flo_w3replay::Record;
use flo_w3replay::{
  GameInfo, PlayerChatMessage, PlayerInfo, PlayerLeft, ProtoBufPayload, RacePref, ReplayEncoder,
  SlotInfo, TimeSlot, TimeSlotAck,
};
use std::io::{Seek, Write};

const FLO_OB_SLOT: usize = 23;
const FLO_PLAYER_ID: u8 = index_to_player_id(FLO_OB_SLOT);

pub struct GenerateReplayOptions {
  pub game: flo_types::observer::GameInfo,
  pub archive: Bytes,
  pub include_chats: bool,
}

pub async fn generate_replay<W>(
  GenerateReplayOptions {
    game,
    archive,
    include_chats,
  }: GenerateReplayOptions,
  w: W,
) -> Result<()>
where
  W: Write + Seek,
{
  let occupied_slots: Vec<(usize, _)> = game
    .slots
    .iter()
    .enumerate()
    .filter(|(_, slot)| slot.settings.status == SlotStatus::Occupied)
    .collect();

  let (first_player_info, first_player_name) = occupied_slots
    .get(0)
    .and_then(|(i, slot)| {
      let name = slot.player.as_ref().map(|p| p.name.clone())?;
      Some((PlayerInfo::new(index_to_player_id(*i), &name), name))
    })
    .unwrap();
  let first_player_id = first_player_info.id;

  let game_info = GameInfo::new(
    first_player_info,
    &game.name,
    flo_w3gs::game::GameSettings::new(
      Default::default(),
      flo_w3gs::game::GameSettingsMap {
        path: game.map.path.clone(),
        width: 0,
        height: 0,
        sha1: {
          let mut value = [0_u8; 20];
          value.copy_from_slice(&game.map.sha1[..]);
          value
        },
        checksum: 0xFFFFFFFF,
      },
    ),
  );

  let flo_ob_slot_occupied = occupied_slots
    .iter()
    .find(|(idx, _)| *idx == FLO_OB_SLOT)
    .is_some();

  if flo_ob_slot_occupied {
    return Err(Error::FloObserverSlotOccupied);
  }

  let mut player_infos = vec![];
  let mut player_skins = vec![];
  let mut player_profiles = vec![];
  let mut active_player_ids = vec![];

  let mut is_first_player = true;
  for (i, slot) in game.slots.iter().enumerate() {
    if let Some(ref p) = slot.player {
      let player_id = index_to_player_id(i);
      active_player_ids.push(player_id);
      if is_first_player {
        is_first_player = false;
      } else {
        player_infos.push(PlayerInfo::new(player_id, p.name.as_str()));
      }
      player_skins.push(ProtoBufPayload::new(PlayerSkinsMessage {
        player_id: player_id as _,
        ..Default::default()
      }));
      player_profiles.push(ProtoBufPayload::new(PlayerProfileMessage {
        player_id: player_id as _,
        battle_tag: p.name.clone(),
        portrait: "p042".to_string(),
        ..Default::default()
      }));
    }
  }

  player_infos.push(PlayerInfo::new(FLO_PLAYER_ID, "FLO"));
  player_profiles.push(ProtoBufPayload::new(PlayerProfileMessage::new(
    FLO_PLAYER_ID,
    "FLO",
  )));

  let mut records = vec![];

  // 0 GameInfo
  records.push(Record::GameInfo(game_info));

  // 1 PlayerInfo
  for item in player_infos {
    records.push(Record::PlayerInfo(flo_w3replay::PlayerInfoRecord {
      player_info: item,
      unknown: 0,
    }));
  }

  // 2 ProtoBuf PlayerSkins
  for item in player_skins {
    records.push(Record::ProtoBuf(item));
  }

  // 3 ProtoBuf PlayerProfile
  for item in player_profiles {
    records.push(Record::ProtoBuf(item));
  }

  // special messages
  {
    let (my_id, my_name) = if flo_ob_slot_occupied {
      (first_player_id, first_player_name.clone())
    } else {
      (index_to_player_id(FLO_OB_SLOT), "FLO".to_string())
    };

    // ProtoBuf PlayerSkinsMessage

    records.push(Record::ProtoBuf(ProtoBufPayload::new(PlayerSkinsMessage {
      player_id: my_id as _,
      ..Default::default()
    })));

    // ProtoBuf PlayerUnknown5
    records.push(Record::ProtoBuf(ProtoBufPayload::new(
      PlayerUnknown5Message {
        player_id: my_id as _,
        unknown_1: 1,
      },
    )));

    let ack_player_profile_count =
      if flo_ob_slot_occupied { 0 } else { 1 } + occupied_slots.len() - 1;
    let msg = PlayerProfileMessage::new(my_id, &my_name);
    for _ in 0..ack_player_profile_count {
      records.push(Record::ProtoBuf(ProtoBufPayload::new(msg.clone())))
    }
  }

  // 4 SlotInfo
  {
    let mut b = SlotInfo::build();
    let mut slot_info = b
      .random_seed(game.random_seed)
      .num_slots(24)
      .num_players(
        occupied_slots
          .iter()
          .filter(|(_, slot)| slot.settings.team != 24 && slot.player.is_some())
          .count(),
      )
      .build();

    for (i, player_slot) in &occupied_slots {
      use flo_w3gs::slot::SlotStatus;
      let slot = slot_info.slot_mut(*i).expect("always has 24 slots");

      if player_slot.player.is_some() {
        slot.player_id = index_to_player_id(*i);
        slot.slot_status = SlotStatus::Occupied;
        slot.race = player_slot.settings.race.into();
        slot.color = player_slot.settings.color as u8;
        slot.team = player_slot.settings.team as u8;
        slot.handicap = player_slot.settings.handicap as u8;
        slot.download_status = 100;
      } else {
        slot.computer = true;
        slot.computer_type = player_slot.settings.computer.into();
        slot.slot_status = SlotStatus::Occupied;
        slot.race = player_slot.settings.race.into();
        slot.color = player_slot.settings.color as u8;
        slot.team = player_slot.settings.team as u8;
        slot.handicap = player_slot.settings.handicap as u8;
        slot.download_status = 100;
      }
    }

    if !flo_ob_slot_occupied {
      use flo_w3gs::slot::SlotStatus;
      let slot = slot_info
        .slot_mut(FLO_OB_SLOT)
        .expect("always has 24 slots");

      slot.player_id = index_to_player_id(FLO_OB_SLOT);
      slot.slot_status = SlotStatus::Occupied;
      slot.race = RacePref::RANDOM;
      slot.color = 0;
      slot.team = 24;
    }

    records.push(Record::SlotInfo(slot_info));
  }

  // 5 CountDownStart
  records.push(Record::CountDownStart(Default::default()));

  // 6 CountDownEnd
  records.push(Record::CountDownEnd(Default::default()));

  // 7 GameStart
  records.push(Record::GameStart(Default::default()));

  let rdr = GameDataArchiveReader::open_bytes(&archive).await?;
  let archive_records = rdr.records().collect_vec().await?;

  tracing::debug!(
    "archive: size: {}, records: {}",
    archive.len(),
    archive_records.len()
  );

  // archive records
  for r in archive_records {
    match r {
      GameRecordData::W3GS(p) => match p.type_id() {
        W3GSPacketTypeId::PlayerLeft => {
          let payload: flo_w3gs::protocol::leave::PlayerLeft = p.decode_simple()?;
          records.push(Record::PlayerLeft(PlayerLeft {
            reason: payload.reason,
            player_id: payload.player_id,
            // All guesses, referenced w3g_format.txt
            result: match payload.reason {
              LeaveReason::LeaveDisconnect => 0x01,
              LeaveReason::LeaveLost => 0x07,
              LeaveReason::LeaveLostBuildings => 0x08,
              LeaveReason::LeaveWon => 0x09,
              _ => 0x0D,
            },
            unknown: 2,
          }));
          active_player_ids.retain(|id| *id != payload.player_id);
        }
        W3GSPacketTypeId::ChatFromHost => {
          let payload: flo_w3gs::protocol::chat::ChatFromHost = p.decode_simple()?;
          if include_chats && payload.0.to_players.contains(&FLO_PLAYER_ID) {
            records.push(Record::ChatMessage(PlayerChatMessage {
              player_id: payload.from_player(),
              message: payload.0.message,
            }));
          }
        }
        W3GSPacketTypeId::IncomingAction => {
          let payload: flo_w3gs::protocol::action::IncomingAction = p.decode_payload()?;
          records.push(Record::TimeSlot(TimeSlot {
            time_increment_ms: payload.0.time_increment_ms,
            actions: payload.0.actions,
          }))
        }
        _ => {}
      },
      GameRecordData::StartLag(_) => {}
      GameRecordData::StopLag(_) => {}
      GameRecordData::GameEnd => {}
      GameRecordData::TickChecksum { checksum, .. } => {
        records.push(Record::TimeSlotAck(TimeSlotAck::new(checksum)))
      }
      GameRecordData::RTTStats(_) => {}
    }
  }

  for player_id in active_player_ids {
    records.push(Record::PlayerLeft(PlayerLeft {
      reason: LeaveReason::LeaveDisconnect,
      player_id,
      result: 0x0D,
      unknown: 2,
    }));
  }

  let mut encoder = ReplayEncoder::new(&game.game_version, 0x8000, w)?;
  encoder.encode_records(records.iter())?;
  encoder.finish()?;

  Ok(())
}

const fn index_to_player_id(index: usize) -> u8 {
  return (index + 1) as u8;
}
