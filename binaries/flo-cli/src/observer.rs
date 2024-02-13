use flo_observer_archiver::{ArchiverOptions, Fetcher};
use flo_replay::{generate_replay, GenerateReplayOptions, ReplayChatPolicy};
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
        let client = flo_client::start_ws(flo_client::StartConfig {
          stats_host: ENV.stats_host.clone().into(),
          ..Default::default()
        })
        .await?;
        client.watch(token).await?;
        client.serve().await;
      }
      Command::GenerateReplay { game_id } => {
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

        tracing::info!("game name = {}, version = {}", game.name, game.game_version);

        tracing::info!("fetching game archive...");

        let opts = ArchiverOptions {
          aws_s3_bucket: ENV.aws_s3_bucket.clone().unwrap(),
          aws_access_key_id: ENV.aws_access_key_id.clone().unwrap(),
          aws_secret_access_key: ENV.aws_secret_access_key.clone().unwrap(),
          aws_s3_region: ENV.aws_s3_region.clone().unwrap(),
        };

        let fetcher = Fetcher::new(opts).unwrap();
        let archive = fetcher.fetch(game_id).await.unwrap();
        tracing::info!("archive: size: {}", archive.len());

        tracing::info!("encoding replay...");

        let file = std::fs::File::create(format!("{}.w3g", game_id)).unwrap();
        let w = std::io::BufWriter::new(file);

        generate_replay(
          GenerateReplayOptions {
            game,
            archive,
            chat_policy: ReplayChatPolicy::IncludeAllChats,
          },
          w,
        )
        .await
        .unwrap();

        tracing::info!("done");
      }
    }
    Ok(())
  }
}
