use flo_net::w3gs::W3GSPacketTypeId;
use flo_observer::record::GameRecordData;
use flo_observer_archiver::{ArchiverOptions, Fetcher};
use flo_observer_fs::GameDataArchiveReader;
use flo_w3gs::player::{PlayerProfileMessage, PlayerSkinsMessage, PlayerUnknown5Message};
use flo_w3replay::{
  GameInfo, PlayerChatMessage, PlayerInfo, PlayerLeft, ProtoBufPayload, RacePref, ReplayEncoder,
  SlotInfo, TimeSlot, TimeSlotAck,
};
use s2_grpc_utils::S2ProtoUnpack;
use structopt::StructOpt;

use crate::{env::ENV, grpc::get_grpc_client, Result};

#[derive(Debug, StructOpt)]
pub enum Command {
  Token {
    game_id: i32,
  },
  Watch {
    game_id: i32,
    delay_secs: Option<i64>,
  },
  GenerateReplay {
    game_id: i32,
  },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::Token { game_id } => {
        let token = flo_observer::token::create_observer_token(game_id, None)?;
        println!("{}", token)
      }
      Command::Watch {
        game_id,
        delay_secs,
      } => {
        let token = flo_observer::token::create_observer_token(game_id, delay_secs)?;
        let client = flo_client::start(flo_client::StartConfig {
          stats_host: ENV.stats_host.clone().into(),
          ..Default::default()
        })
        .await?;
        client.watch(token).await?;
        client.serve().await;
      }
      Command::GenerateReplay { game_id } => {
        use flo_types::game::SlotStatus;
        use flo_w3replay::Record;

        tracing::info!("fetching game information...");

        let ctrl = get_grpc_client().await;
        let game = ctrl
          .clone()
          .get_game(flo_grpc::controller::GetGameRequest { game_id })
          .await?
          .into_inner()
          .game
          .unwrap();

        let game = flo_types::observer::GameInfo::unpack(game).unwrap();

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
              // path: game.map.path.clone(),
              path: r#"Maps/W3Champions\\w3c_s12.2_VanguardPoint_v1.3.w3x"#.to_string(),
              width: 0,
              height: 0,
              sha1: {
                let mut value = [0_u8; 20];
                value.copy_from_slice(&game.map.sha1[..]);
                value
              },
              checksum: game.map.checksum,
            },
          ),
        );

        tracing::info!("game name = {}, version = {}", game.name, game.game_version);

        tracing::info!("fetching game archive...");

        let opts = ArchiverOptions {
          aws_s3_bucket: ENV.aws_s3_bucket.clone().unwrap(),
          aws_access_key_id: ENV.aws_access_key_id.clone().unwrap(),
          aws_secret_access_key: ENV.aws_secret_access_key.clone().unwrap(),
          aws_s3_region: ENV.aws_s3_region.clone().unwrap(),
        };

        let fetcher = Fetcher::new(opts).unwrap();
        let bytes = fetcher.fetch(game_id).await.unwrap();
        let rdr = GameDataArchiveReader::open_bytes(&bytes).await.unwrap();
        let archive_records = rdr.records().collect_vec().await.unwrap();
        tracing::info!(
          "archive: size: {}, records: {}",
          bytes.len(),
          archive_records.len()
        );

        fn index_to_player_id(index: usize) -> u8 {
          return (index + 1) as u8;
        }

        const FLO_OB_SLOT: usize = 23;

        let flo_ob_slot_occupied = occupied_slots
          .iter()
          .find(|(idx, _)| *idx == FLO_OB_SLOT)
          .is_some();

        let mut player_infos = vec![];
        let mut player_skins = vec![];
        let mut player_profiles = vec![];

        let mut is_first_player = true;
        for (i, slot) in game.slots.iter().enumerate() {
          if let Some(ref p) = slot.player {
            let player_id = index_to_player_id(i);
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

        if !flo_ob_slot_occupied {
          let ob_player_id = index_to_player_id(FLO_OB_SLOT);
          player_infos.push(PlayerInfo::new(ob_player_id, "FLO"));
          player_profiles.push(ProtoBufPayload::new(PlayerProfileMessage::new(
            ob_player_id,
            "FLO",
          )));
        }

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

        // archive records
        for r in archive_records {
          match r {
            GameRecordData::W3GS(p) => match p.type_id() {
              W3GSPacketTypeId::PlayerLeft => {
                let payload: flo_w3gs::protocol::leave::PlayerLeft = p.decode_simple().unwrap();
                records.push(Record::PlayerLeft(PlayerLeft {
                  reason: payload.reason,
                  player_id: payload.player_id,
                  result: 13, // ?
                  unknown: 2,
                }))
              }
              W3GSPacketTypeId::ChatFromHost => {
                let payload: flo_w3gs::protocol::chat::ChatFromHost = p.decode_simple().unwrap();
                if payload
                  .0
                  .to_players
                  .contains(&index_to_player_id(FLO_OB_SLOT))
                {
                  records.push(Record::ChatMessage(PlayerChatMessage {
                    player_id: payload.from_player(),
                    message: payload.0.message,
                  }));
                }
              }
              W3GSPacketTypeId::IncomingAction => {
                let payload: flo_w3gs::protocol::action::IncomingAction =
                  p.decode_payload().unwrap();
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

        tracing::info!("total replay records: {}", records.len());
        tracing::info!("encoding replay...");

        let file = std::fs::File::create(format!("{}.w3g", game_id)).unwrap();
        let w = std::io::BufWriter::new(file);
        let mut encoder = ReplayEncoder::new(&game.game_version, 0x8000, w).unwrap();
        encoder.encode_records(records.iter()).unwrap();
        encoder.finish().unwrap();

        tracing::info!("done");
      }
    }
    Ok(())
  }
}
