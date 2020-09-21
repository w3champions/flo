use super::{PlayerRegistry, PlayerState};
use crate::error::*;
use flo_net::packet::{Frame};
use flo_state::{async_trait, Addr, Context, Handler, Message};
use std::collections::{BTreeMap};

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
    let mut broken = false;
    if let Some(entry) = entry {
      for frame in frames {
        if !entry.sender.try_send(frame) {
          broken = true;
          break;
        }
      }
      broken
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
pub struct PlayerPacketSender(Addr<PlayerRegistry>);
impl PlayerPacketSender {
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
}

impl From<Addr<PlayerRegistry>> for PlayerPacketSender {
  fn from(value: Addr<PlayerRegistry>) -> Self {
    Self(value)
  }
}
