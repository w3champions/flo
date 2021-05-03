use crate::error::{Error, Result};
use flo_lan::LanGame;
use flo_w3gs::chat::ChatFromHost;
use flo_w3gs::game::PlayerLoaded;
use flo_w3gs::leave::PlayerLeft;
use flo_w3gs::net::W3GSStream;
use flo_w3gs::protocol::action::{IncomingAction, OutgoingAction, OutgoingKeepAlive};
use flo_w3gs::protocol::constants::{
  LeaveReason, PacketTypeId, ProtoBufMessageTypeId, RejectJoinReason,
};
use flo_w3gs::protocol::game::GameLoadedSelf;
use flo_w3gs::protocol::join::*;
use flo_w3gs::protocol::leave::{LeaveReq, PlayerKicked};
use flo_w3gs::protocol::map::MapSize;
use flo_w3gs::protocol::packet::{Packet, ProtoBufPayload};
use flo_w3gs::protocol::player::{
  PlayerInfo, PlayerProfileMessage, PlayerSkinsMessage, PlayerUnknown5Message,
};
use flo_w3gs::protocol::slot::SlotInfo;
use flo_w3map::{MapChecksum, W3Map};
use flo_w3storage::W3Storage;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::{interval_at, Instant};

#[derive(Error, Debug)]
pub enum PlayerEmulatorError {
  #[error("join rejected: {0:?}")]
  JoinRejected(RejectJoinReason),
  #[error("map checksum mismatch")]
  MapChecksumMismatch,
  #[error("kicked")]
  Kicked,
}

enum Cmd {
  Leave,
}

pub struct PlayerEmulator {
  stream: W3GSStream,
  _info: JoinInfo,
  tx: Sender<Cmd>,
  rx: Option<Receiver<Cmd>>,
}

impl PlayerEmulator {
  pub fn check_map(game: &LanGame, storage: &W3Storage) -> Result<MapChecksum> {
    let map_path = game.game_info.data.settings.map_path.to_string_lossy();
    tracing::debug!("calculating map checksum: {}", map_path);
    let (_, map_checksum) = W3Map::open_storage_with_checksum(storage, &map_path)?;
    if game.game_info.data.settings.map_sha1 != map_checksum.sha1 {
      return Err(PlayerEmulatorError::MapChecksumMismatch.into());
    }
    Ok(map_checksum)
  }

  pub async fn join(game: &LanGame, map_checksum: MapChecksum, player_name: &str) -> Result<Self> {
    tracing::debug!(
      "connecting {:?} with secret {:x}...",
      game.addr,
      game.game_info.secret
    );
    let mut stream = W3GSStream::connect(game.addr).await?;
    let (tx, rx) = channel(1);

    let info = JoinHandler {
      player_name,
      game,
      map_checksum,
      stream: &mut stream,
    }
    .run()
    .await?;

    Ok(Self {
      stream,
      _info: info,
      tx,
      rx: Some(rx),
    })
  }

  pub async fn run(mut self) -> Result<()> {
    let mut rx = self.rx.take().unwrap();
    let mut interval = interval_at(
      Instant::now() + Duration::from_secs(1),
      Duration::from_millis(300),
    );
    loop {
      tokio::select! {
        next = self.stream.recv() => {
          if let Some(packet) = next? {
            self.handle_packet(packet).await?;
          } else {
            return Ok(())
          }
        }
        _ = interval.tick() => {
          self.stream.send(Packet::with_payload(
            OutgoingAction::new(&[0x33, 0x99, 0xFF, 0x00])
          )?).await?;
        }
        Some(cmd) = rx.recv() => {
          match cmd {
            Cmd::Leave => {
              self.stream.send(
                Packet::simple(LeaveReq::new(LeaveReason::LeaveLost))?
              ).await?;
            }
          }
        }
      }
    }
  }

  pub fn handle(&self) -> PlayerEmulatorHandle {
    PlayerEmulatorHandle(self.tx.clone())
  }

  async fn handle_packet(&mut self, mut packet: Packet) -> Result<()> {
    // tracing::debug!("recv: {:?}", packet.type_id());
    match packet.type_id() {
      PacketTypeId::IncomingAction => {
        let payload: IncomingAction = packet.decode_payload()?;
        tracing::debug!("incoming action: {:?}", payload);
        self
          .stream
          .send(Packet::simple(OutgoingKeepAlive {
            unknown: 0,
            checksum: 0,
          })?)
          .await?;
      }
      PacketTypeId::PlayerLoaded => {
        let payload: PlayerLoaded = packet.decode_simple()?;
        tracing::debug!("player loaded: {:?}", payload);
      }
      PacketTypeId::PlayerLeft => {
        let payload: PlayerLeft = packet.decode_simple()?;
        tracing::debug!("player left: {:?}", payload);
      }
      PacketTypeId::ChatFromHost => {
        let payload: ChatFromHost = packet.decode_simple()?;
        tracing::debug!("chat from host: {:?}", payload);
      }
      PacketTypeId::ProtoBuf => {
        let payload: ProtoBufPayload = packet.decode_simple()?;
        tracing::debug!("recv protobuf: {:?}", payload.type_id);
      }
      PacketTypeId::PingFromHost => {
        packet.header.type_id = PacketTypeId::PongToHost;
        self.stream.send(packet).await?;
      }
      _ => return Err(Error::UnexpectedW3GSPacket(packet)),
    }
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct PlayerEmulatorHandle(Sender<Cmd>);

impl PlayerEmulatorHandle {
  pub async fn leave(&self) {
    self.0.send(Cmd::Leave).await.ok();
  }
}

struct JoinInfo {
  _player_id: u8,
}

struct JoinHandler<'a> {
  player_name: &'a str,
  game: &'a LanGame,
  map_checksum: MapChecksum,
  stream: &'a mut W3GSStream,
}

impl<'a> JoinHandler<'a> {
  async fn run(mut self) -> Result<JoinInfo> {
    let req_join = Packet::simple(ReqJoin::new(
      self.player_name,
      self.game.id,
      self.game.game_info.secret,
    ))?;

    self.stream.send(req_join).await?;
    let reply = self
      .stream
      .recv()
      .await?
      .ok_or_else(|| Error::StreamClosed)?;
    let slots = match reply.type_id() {
      PacketTypeId::SlotInfoJoin => reply.decode_simple::<SlotInfoJoin>()?,
      PacketTypeId::RejectJoin => {
        let payload = reply.decode_simple::<RejectJoin>()?;
        return Err(PlayerEmulatorError::JoinRejected(payload.reason).into());
      }
      _ => return Err(Error::UnexpectedW3GSPacket(reply)),
    };

    let player_id = slots.player_id;
    tracing::debug!("player_id: {}", player_id);

    let profile = Packet::simple(ProtoBufPayload::new(PlayerProfileMessage {
      player_id: player_id as u32,
      battle_tag: self.player_name.to_string(),
      ..Default::default()
    }))?;

    let skins = Packet::simple(ProtoBufPayload::new(PlayerSkinsMessage {
      player_id: player_id as u32,
      ..Default::default()
    }))?;

    let unk5 = Packet::simple(ProtoBufPayload::new(PlayerUnknown5Message {
      player_id: player_id as u32,
      ..Default::default()
    }))?;

    self
      .stream
      .send_all(vec![profile.clone(), skins, unk5])
      .await?;

    loop {
      tokio::select! {
        next = self.stream.recv() => {
          match next? {
            Some(pkt) => {
              if self.handle_packet(pkt, player_id, &profile).await? {
                break;
              }
            },
            None => {
              return Err(Error::StreamClosed)
            },
          }
        }
      }
    }

    Ok(JoinInfo {
      _player_id: player_id,
    })
  }

  async fn handle_packet(
    &mut self,
    mut packet: Packet,
    player_id: u8,
    profile: &Packet,
  ) -> Result<bool> {
    match packet.type_id() {
      PacketTypeId::CountDownStart => {}
      PacketTypeId::CountDownEnd => {
        self.stream.send(Packet::simple(GameLoadedSelf)?).await?;
        return Ok(true);
      }
      PacketTypeId::PlayerInfo => {
        let payload: PlayerInfo = packet.decode_simple()?;
        tracing::debug!("player info: {:?}", payload);
      }
      PacketTypeId::SlotInfo => {
        let payload: SlotInfo = packet.decode_simple()?;
        tracing::debug!("slot info: {:?}", payload);
      }
      PacketTypeId::MapCheck => {
        self
          .stream
          .send(Packet::simple(MapSize::new(
            self.map_checksum.file_size as u32,
          ))?)
          .await?;
      }
      PacketTypeId::PlayerLeft => {
        let payload: PlayerLeft = packet.decode_simple()?;
        tracing::debug!("player left: {:?}", payload);
      }
      PacketTypeId::PlayerKicked => {
        let payload: PlayerKicked = packet.decode_simple()?;
        tracing::debug!("player kicked: {:?}", payload);
        return Err(PlayerEmulatorError::Kicked.into());
      }
      PacketTypeId::ChatFromHost => {
        let payload: ChatFromHost = packet.decode_simple()?;
        tracing::debug!("chat from host: {:?}", payload);
      }
      PacketTypeId::ProtoBuf => {
        let payload: ProtoBufPayload = packet.decode_simple()?;
        tracing::debug!("recv protobuf: {:?}", payload.type_id);
        if payload.type_id == ProtoBufMessageTypeId::PlayerProfile {
          let msg = payload.decode_message::<PlayerProfileMessage>()?;
          if msg.player_id != player_id as u32 {
            self.stream.send(profile.clone()).await?;
          }
        }
      }
      PacketTypeId::PingFromHost => {
        packet.header.type_id = PacketTypeId::PongToHost;
        self.stream.send(packet).await?;
      }
      _ => return Err(Error::UnexpectedW3GSPacket(packet)),
    }
    Ok(false)
  }
}

#[tokio::test]
async fn test_emu_join() -> Result<()> {
  use std::time::Duration;

  flo_log_subscriber::init();

  let storage = W3Storage::from_env()?;
  let mut games = flo_lan::search_lan_games(Duration::from_secs(1)).await;
  let game = games.first_mut().unwrap();
  tracing::info!(
    "joining lan game: {} at {:?}",
    game.game_info.name.to_string_lossy(),
    game.addr
  );

  let map = PlayerEmulator::check_map(game, &storage)?;
  let mut player = PlayerEmulator::join(game, map, "TEST").await?;
  player.run().await?;
  Ok(())
}
