pub mod diag;
pub mod game;

use std::collections::HashMap;
use std::sync::Arc;

use game::LanGame;

use crate::controller::ControllerClient;
use crate::error::*;
use crate::node::stream::NodeStreamEvent;
use crate::node::NodeInfo;
use crate::platform::{CalcMapChecksum, GetClientPlatformInfo, Platform};
use crate::StartConfig;
use flo_state::{
  async_trait, Actor, Addr, Context, Deferred, Handler, Message, RegistryRef, Service,
};
use flo_types::node::{NodeGameStatus, SlotClientStatus};
use flo_types::game::LocalGameInfo;

pub struct Lan {
  platform: Addr<Platform>,
  client: Deferred<ControllerClient, StartConfig>,
  active_game: Option<LanGame>,
}

impl Actor for Lan {}

#[async_trait]
impl Service<StartConfig> for Lan {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve().await?;
    Ok(Lan {
      platform,
      client: registry.deferred(),
      active_game: None,
    })
  }
}

pub struct ReplaceLanGame {
  pub my_player_id: i32,
  pub node: Arc<NodeInfo>,
  pub player_token: Vec<u8>,
  pub game: Arc<LocalGameInfo>,
}

impl Message for ReplaceLanGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<ReplaceLanGame> for Lan {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    ReplaceLanGame {
      my_player_id,
      node,
      player_token,
      game,
    }: ReplaceLanGame,
  ) -> <ReplaceLanGame as Message>::Result {
    let game_id = game.game_id;
    if self
      .active_game
      .as_ref()
      .map(|g| g.is_same_game(game_id, my_player_id))
      == Some(true)
    {
      tracing::debug!("skip create: same game");
      return Ok(());
    }

    let checksum = self
      .platform
      .send(CalcMapChecksum {
        path: game.map_path.clone(),
      })
      .await??;

    if checksum.sha1 == game.map_sha1 {
      if let Some(last_game) = self.active_game.take() {
        last_game.shutdown();
      }

      let game_version = self
        .platform
        .send(GetClientPlatformInfo::default())
        .await?
        .map_err(|_| Error::War3NotLocated)?
        .version;

      let lan_game = LanGame::create(
        game_version,
        my_player_id,
        node,
        player_token,
        game,
        checksum,
        self.client.resolve().await?,
      )
      .await?;
      tracing::info!(player_id = my_player_id, game_id, "lan game created.");
      self.active_game = Some(lan_game);
    } else {
      self.active_game.take();
      return Err(Error::MapChecksumMismatch);
    }
    Ok(())
  }
}

pub struct UpdateLanGameStatus {
  pub game_id: i32,
  pub status: NodeGameStatus,
  pub updated_player_game_client_status_map: HashMap<i32, SlotClientStatus>,
}

impl Message for UpdateLanGameStatus {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<UpdateLanGameStatus> for Lan {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateLanGameStatus {
      game_id,
      status,
      updated_player_game_client_status_map,
    }: UpdateLanGameStatus,
  ) -> <UpdateLanGameStatus as Message>::Result {
    let game = if let Some(game) = self.active_game.as_mut() {
      game
    } else {
      return Err(Error::NotInGame);
    };

    if game.game_id() != game_id {
      tracing::error!("UpdateLanGameStatus: game id mismatch");
      return Ok(());
    }

    for (player_id, status) in updated_player_game_client_status_map {
      game.update_player_status(player_id, status).await;
    }
    game.update_game_status(status).await;

    Ok(())
  }
}

pub struct UpdateLanGamePlayerStatus {
  pub game_id: i32,
  pub player_id: i32,
  pub status: SlotClientStatus,
}

impl Message for UpdateLanGamePlayerStatus {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<UpdateLanGamePlayerStatus> for Lan {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateLanGamePlayerStatus {
      game_id,
      player_id,
      status,
    }: UpdateLanGamePlayerStatus,
  ) -> <UpdateLanGamePlayerStatus as Message>::Result {
    let game = if let Some(game) = self.active_game.as_mut() {
      game
    } else {
      return Err(Error::NotInGame);
    };

    if game.game_id() != game_id {
      tracing::warn!("UpdateLanGamePlayerStatus: game id mismatch");
      return Ok(());
    }

    game.update_player_status(player_id, status).await;

    Ok(())
  }
}

pub struct StopLanGame {
  pub game_id: i32,
}

impl Message for StopLanGame {
  type Result = ();
}

#[async_trait]
impl Handler<StopLanGame> for Lan {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    StopLanGame { game_id }: StopLanGame,
  ) -> <StopLanGame as Message>::Result {
    if self.active_game.as_ref().map(|g| g.game_id()) == Some(game_id) {
      if let Some(game) = self.active_game.take() {
        game.shutdown();
      }
    }
  }
}

pub struct KillLanGame;

impl Message for KillLanGame {
  type Result = ();
}

#[async_trait]
impl Handler<KillLanGame> for Lan {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: KillLanGame,
  ) -> <KillLanGame as Message>::Result {
    self.active_game.take();
  }
}

pub fn get_lan_game_name(game_name: &str, player_id: i32) -> String {
  format!("{}-{}", game_name, player_id)
}

#[derive(Debug)]
pub enum LanEvent {
  LanGameDisconnected {
    game_id: i32,
  },
  NodeStreamEvent {
    game_id: i32,
    inner: NodeStreamEvent,
  },
}

impl Message for LanEvent {
  type Result = ();
}
