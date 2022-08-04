use flo_w3gs::player::{PlayerProfileMessage, PlayerSkinsMessage};
use flo_w3replay::{GameInfo, GameSettings, PlayerInfo, ProtoBufPayload};
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
        use flo_util::binary::IntoCStringLossy;
        let ctrl = get_grpc_client().await;
        let game = ctrl
          .clone()
          .get_game(flo_grpc::controller::GetGameRequest { game_id })
          .await?
          .into_inner()
          .game
          .unwrap();
        let game = flo_types::observer::GameInfo::unpack(game).unwrap();
        let game_info = GameInfo::new(
          &game.name,
          &game.map.path,
          {
            let mut value = [0_u8; 20];
            value.copy_from_slice(&game.map.sha1[..]);
            value
          },
          game.map.checksum,
        );

        fn index_to_player_id(index: usize) -> u8 {
          return (index + 1) as u8;
        }

        let mut player_infos = vec![];
        let mut player_skins = vec![];
        let mut player_profiles = vec![];

        for (i, slot) in game.slots.iter().enumerate() {
          if let Some(ref p) = slot.player {
            let player_id = index_to_player_id(i);
            player_infos.push(PlayerInfo::new(player_id, p.name.as_str()));
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
      }
    }
    Ok(())
  }
}
