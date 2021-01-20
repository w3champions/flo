use flo_grpc::controller::*;
use structopt::StructOpt;

use crate::game::create_game;
use crate::grpc::get_grpc_client;
use crate::Result;
use flo_controller::player::PlayerSource;

#[derive(Debug, StructOpt)]
pub enum Command {
  UpsertPlayer {
    id: String,
    name: Option<String>,
  },
  RunGame {
    player: Vec<i32>,
    #[structopt(long)]
    ob: Option<i32>,
  },
  StartGame {
    id: i32,
  },
  CancelGame {
    id: i32,
  },
  ListNodes
}

impl Command {
  pub async fn run(self) -> Result<()> {
    let mut client = get_grpc_client().await;
    match self {
      Command::UpsertPlayer { id, name } => {
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
      Command::RunGame { player, ob } => {
        let game_id = create_game(player, ob).await?;
        tracing::info!(game_id);
        let res = client
          .start_game_as_bot(StartGameAsBotRequest { game_id })
          .await?
          .into_inner();
        tracing::info!("start game: {:?}", res);
      }
      Command::StartGame { id } => {
        let res = client
          .start_game_as_bot(StartGameAsBotRequest { game_id: id })
          .await?
          .into_inner();
        tracing::info!("start game: {:?}", res);
      }
      Command::CancelGame { id } => {
        client
          .cancel_game_as_bot(CancelGameAsBotRequest { game_id: id })
          .await?;
      }
      Command::ListNodes => {
        let res = client.list_nodes(()).await;
        tracing::info!("nodes: {:?}", res);
      }
    }

    Ok(())
  }
}
