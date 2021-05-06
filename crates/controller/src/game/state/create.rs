use crate::error::{Error, Result};
use crate::game::db::{CreateGameAsBotParams, CreateGameParams};
use crate::game::state::registry::Register;
use crate::game::state::GameRegistry;
use crate::game::{Game, GameStatus};
use flo_state::{async_trait, Context, Handler, Message};

pub struct CreateGame {
  pub params: CreateGameParams,
}

impl Message for CreateGame {
  type Result = Result<Game>;
}

#[async_trait]
impl Handler<CreateGame> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CreateGame { params }: CreateGame,
  ) -> <CreateGame as Message>::Result {
    let player_id = params.player_id;
    let game = self
      .db
      .exec(move |conn| crate::game::db::create(conn, params))
      .await?;

    self.register(Register {
      id: game.id,
      status: GameStatus::Preparing,
      host_player: game.created_by.id,
      players: game.get_player_ids(),
      node_id: None,
    });

    self
      .players
      .player_replace_game(player_id, game.clone(), vec![])
      .await?;

    Ok(game)
  }
}

pub struct CreateGameAsBot {
  pub api_client_id: i32,
  pub api_player_id: i32,
  pub params: CreateGameAsBotParams,
}

impl Message for CreateGameAsBot {
  type Result = Result<Game>;
}

#[async_trait]
impl Handler<CreateGameAsBot> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CreateGameAsBot {
      api_client_id,
      api_player_id,
      params,
    }: CreateGameAsBot,
  ) -> <CreateGameAsBot as Message>::Result {
    let (mut game, player_ids, mute_list_map) = self
      .db
      .exec(move |conn| {
        let game = crate::game::db::create_as_bot(conn, api_client_id, api_player_id, params)?;
        let player_ids = game.get_player_ids();
        let mute_list_map = crate::player::db::get_mute_list_map(conn, &player_ids)?;
        Ok::<_, Error>((game, player_ids, mute_list_map))
      })
      .await?;

    if game.mask_player_names {
      for (idx, slot) in game.slots.iter_mut().enumerate() {
        slot
          .player
          .as_mut()
          .map(|v| v.name = format!("Player {}", idx + 1));
      }
    }

    self.register(Register {
      id: game.id,
      status: GameStatus::Preparing,
      host_player: game.created_by.id,
      players: player_ids.clone(),
      node_id: game.node.as_ref().map(|v| v.id),
    });

    self
      .players
      .players_replace_game(player_ids, game.clone(), mute_list_map)
      .await?;

    Ok(game)
  }
}
