use flo_grpc::controller::*;
use structopt::StructOpt;

use crate::game::create_game;
use crate::grpc::get_grpc_client;
use crate::Result;
use flo_controller::player::PlayerSource;

#[derive(Debug, StructOpt)]
pub enum Command {
  UpsertPlayer { id: String, name: Option<String> },
  CreateGame { player: Vec<i32> },
  StartGame { id: i32 },
}

impl Command {
  pub async fn run(self) -> Result<()> {
    match self {
      Command::UpsertPlayer { id, name } => {
        let mut client = get_grpc_client().await;
        let res = client
          .update_and_get_player(UpdateAndGetPlayerRequest {
            source: PlayerSource::Api as i32,
            name: name.unwrap_or_else(|| format!("Player#{}", id)),
            source_id: id,
            ..Default::default()
          })
          .await?
          .into_inner();
        let player = res.player.unwrap();
        tracing::info!("player id: {}", player.id);
        tracing::info!("token: {}", res.token);
      }
      Command::CreateGame { player } => {
        let game_id = create_game(player).await?;
        tracing::info!(game_id);
      }
      Command::StartGame { id } => {
        let mut client = get_grpc_client().await;
        let res = client
          .start_game_as_bot(StartGameAsBotRequest { game_id: id })
          .await?
          .into_inner();
        tracing::info!("start game: {:?}", res);
      }
    }

    Ok(())
  }
}
