use super::{PlayerRegistry, PlayerState};
use crate::error::*;
use crate::game::Game;
use crate::player::session::get_session_update_packet;
use flo_net::packet::{FloPacket, Frame};
use flo_state::{async_trait, Addr, Context, Handler, Message};
use s2_grpc_utils::S2ProtoPack;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

#[derive(Debug)]
struct Send {
  player_id: i32,
  frames: PlayerFrames,
}

impl Message for Send {
  type Result = ();
}

#[async_trait]
impl Handler<Send> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, Send { player_id, frames }: Send) {
    send_to_player(&mut self.registry, player_id, frames);
  }
}

#[derive(Debug)]
struct BroadcastToAll {
  frames: PlayerFrames,
}

impl Message for BroadcastToAll {
  type Result = ();
}

#[async_trait]
impl Handler<BroadcastToAll> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, BroadcastToAll { frames }: BroadcastToAll) {
    let mut remove_list = vec![];
    for (player_id, state) in self.registry.iter_mut() {
      let remove = { !state.try_send_frames(frames.clone()) };
      if remove {
        let player_id = *player_id;
        tracing::debug!(player_id, "remove broken player sender");
        remove_list.push(player_id);
      }
    }
    for id in remove_list {
      self.registry.remove(&id);
    }
  }
}

#[derive(Debug)]
struct Broadcast {
  players: Vec<i32>,
  frames: PlayerFrames,
}

impl Message for Broadcast {
  type Result = ();
}

#[async_trait]
impl Handler<Broadcast> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, Broadcast { players, frames }: Broadcast) {
    for player_id in players {
      send_to_player(&mut self.registry, player_id, frames.clone());
    }
  }
}

#[derive(Debug)]
struct BroadcastMap {
  map: BTreeMap<i32, PlayerFrames>,
}

impl Message for BroadcastMap {
  type Result = ();
}

#[async_trait]
impl Handler<BroadcastMap> for PlayerRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, BroadcastMap { map }: BroadcastMap) {
    for (player_id, frames) in map {
      send_to_player(&mut self.registry, player_id, frames);
    }
  }
}

pub struct PlayerReplaceGame {
  pub player_id: i32,
  pub game: Game,
}

impl Message for PlayerReplaceGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<PlayerReplaceGame> for PlayerRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    PlayerReplaceGame { player_id, game }: PlayerReplaceGame,
  ) -> Result<()> {
    use flo_net::proto::flo_connect::*;
    let game_id = game.id;

    if let Entry::Occupied(mut entry) = self.registry.entry(player_id) {
      let frames = vec![
        get_session_update_packet(Some(game.id)).encode_as_frame()?,
        PacketGameInfo {
          game: Some(game.pack()?),
        }
        .encode_as_frame()?,
      ];
      entry.get_mut().game_id = Some(game_id);
      if !entry.get_mut().try_send_frames(frames.into()) {
        entry.remove();
      }
    }

    Ok(())
  }
}

pub struct PlayersReplaceGame {
  pub player_ids: Vec<i32>,
  pub game: Game,
}

impl Message for PlayersReplaceGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<PlayersReplaceGame> for PlayerRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    PlayersReplaceGame { player_ids, game }: PlayersReplaceGame,
  ) -> Result<()> {
    use flo_net::proto::flo_connect::*;
    let game_id = game.id;

    let frames = vec![
      get_session_update_packet(Some(game.id)).encode_as_frame()?,
      PacketGameInfo {
        game: Some(game.pack()?),
      }
      .encode_as_frame()?,
    ];

    for player_id in player_ids {
      if let Entry::Occupied(mut entry) = self.registry.entry(player_id) {
        entry.get_mut().game_id = Some(game_id);
        if !entry.get_mut().try_send_frames(frames.clone().into()) {
          entry.remove();
        }
      }
    }

    Ok(())
  }
}

pub struct PlayerLeaveGame {
  pub player_id: i32,
  pub game_id: i32,
}

impl Message for PlayerLeaveGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<PlayerLeaveGame> for PlayerRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    PlayerLeaveGame { player_id, game_id }: PlayerLeaveGame,
  ) -> Result<()> {
    if let Entry::Occupied(mut entry) = self.registry.entry(player_id) {
      if entry.get().game_id == Some(game_id) {
        if !entry
          .get_mut()
          .sender
          .try_send(get_session_update_packet(None).encode_as_frame()?)
        {
          entry.remove();
        } else {
          entry.get_mut().game_id = None;
        }
      } else {
        tracing::debug!(player_id, game_id, "leave game message ignored");
      }
    }

    Ok(())
  }
}

pub struct PlayersLeaveGame {
  pub player_ids: Vec<i32>,
  pub game_id: i32,
}

impl Message for PlayersLeaveGame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<PlayersLeaveGame> for PlayerRegistry {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    PlayersLeaveGame {
      player_ids,
      game_id,
    }: PlayersLeaveGame,
  ) -> Result<()> {
    for player_id in player_ids {
      self
        .handle(ctx, PlayerLeaveGame { player_id, game_id })
        .await?;
    }

    Ok(())
  }
}

#[derive(Debug, Clone)]
pub enum PlayerFrames {
  Single(Frame),
  Multi(Vec<Frame>),
}

impl IntoIterator for PlayerFrames {
  type Item = Frame;
  type IntoIter = PlayerFramesIntoIterator;

  fn into_iter(self) -> Self::IntoIter {
    PlayerFramesIntoIterator { inner: Some(self) }
  }
}

impl From<Frame> for PlayerFrames {
  fn from(value: Frame) -> Self {
    PlayerFrames::Single(value)
  }
}

impl From<Vec<Frame>> for PlayerFrames {
  fn from(value: Vec<Frame>) -> Self {
    PlayerFrames::Multi(value)
  }
}

#[derive(Debug)]
pub struct PlayerFramesIntoIterator {
  inner: Option<PlayerFrames>,
}

impl Iterator for PlayerFramesIntoIterator {
  type Item = Frame;

  fn next(&mut self) -> Option<Self::Item> {
    match self.inner.as_ref() {
      None => return None,
      Some(_) => {}
    }

    match self.inner.take() {
      None => unreachable!(),
      Some(PlayerFrames::Single(frame)) => Some(frame),
      Some(PlayerFrames::Multi(mut frames)) => {
        let mut r = None;
        if !frames.is_empty() {
          r = Some(frames.remove(0))
        }
        if !frames.is_empty() {
          self.inner = Some(PlayerFrames::Multi(frames))
        }
        r
      }
    }
  }
}

fn send_to_player(map: &mut BTreeMap<i32, PlayerState>, player_id: i32, frames: PlayerFrames) {
  let remove = {
    let entry = map.get_mut(&player_id);
    if let Some(entry) = entry {
      !entry.try_send_frames(frames)
    } else {
      false
    }
  };
  if remove {
    tracing::debug!(player_id, "remove broken player sender");
    map.remove(&player_id);
  }
}

#[derive(Clone)]
pub struct PlayerRegistryHandle(Addr<PlayerRegistry>);
impl PlayerRegistryHandle {
  pub async fn send<T>(&self, player_id: i32, frames: T) -> Result<()>
  where
    T: Into<PlayerFrames>,
  {
    self
      .0
      .send(Send {
        player_id,
        frames: frames.into(),
      })
      .await?;
    Ok(())
  }

  pub async fn broadcast_to_all<T>(&self, frames: T) -> Result<()>
  where
    T: Into<PlayerFrames>,
  {
    self
      .0
      .send(BroadcastToAll {
        frames: frames.into(),
      })
      .await?;
    Ok(())
  }

  pub async fn broadcast<T>(&self, players: Vec<i32>, frames: T) -> Result<()>
  where
    T: Into<PlayerFrames>,
  {
    self
      .0
      .send(Broadcast {
        players,
        frames: frames.into(),
      })
      .await?;
    Ok(())
  }

  pub async fn broadcast_map<T>(&self, iter: T) -> Result<()>
  where
    T: IntoIterator<Item = (i32, PlayerFrames)>,
  {
    self
      .0
      .send(BroadcastMap {
        map: iter.into_iter().collect(),
      })
      .await?;
    Ok(())
  }

  pub async fn player_replace_game(&self, player_id: i32, game: Game) -> Result<()> {
    self.0.send(PlayerReplaceGame { player_id, game }).await??;
    Ok(())
  }

  pub async fn players_replace_game(&self, player_ids: Vec<i32>, game: Game) -> Result<()> {
    self
      .0
      .send(PlayersReplaceGame { player_ids, game })
      .await??;
    Ok(())
  }

  pub async fn players_leave_game(&self, player_ids: Vec<i32>, game_id: i32) -> Result<()> {
    self
      .0
      .send(PlayersLeaveGame {
        player_ids,
        game_id,
      })
      .await??;
    Ok(())
  }

  pub async fn player_leave_game(&self, player_id: i32, game_id: i32) -> Result<()> {
    self
      .0
      .send(PlayerLeaveGame { player_id, game_id })
      .await??;
    Ok(())
  }
}

impl From<Addr<PlayerRegistry>> for PlayerRegistryHandle {
  fn from(value: Addr<PlayerRegistry>) -> Self {
    Self(value)
  }
}
