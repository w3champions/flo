use super::types::ObserverGameInfo;
use crate::error::{Error, Result};
use crate::lan::game::slot::{LanSlotInfo, SelfPlayer};
use crate::platform::{OpenMap, Platform};
use flo_lan::MdnsPublisher;
use flo_state::{Actor, Addr};
use flo_util::binary::SockAddr;
use flo_w3gs::constants::{PacketTypeId, ProtoBufMessageTypeId};
use flo_w3gs::game::{GameSettings, GameSettingsMap};
use flo_w3gs::net::{W3GSListener, W3GSStream};
use flo_w3gs::packet::Packet;
use flo_w3gs::protocol::game::{CountDownEnd, CountDownStart};
use flo_w3gs::protocol::join::{ReqJoin, SlotInfoJoin};
use flo_w3gs::protocol::leave::LeaveAck;
use flo_w3gs::protocol::map::{MapCheck, MapSize};
use flo_w3gs::protocol::packet::ProtoBufPayload;
use flo_w3gs::protocol::player::{PlayerInfo, PlayerProfileMessage, PlayerSkinsMessage};
use flo_w3map::MapChecksum;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

pub struct ObserverGameHost {
  ct: CancellationToken,
  tx: Sender<Cmd>,
  port: u16,
}

impl ObserverGameHost {
  pub async fn start(info: ObserverGameInfo, platform: Addr<Platform>) -> Result<Self> {
    let ct = CancellationToken::new();
    let (tx, rx) = channel(32);

    let map = platform
      .send(OpenMap {
        path: info.map_path.clone(),
      })
      .await??;

    if map.checksum.sha1 != info.map_sha1 {
      return Err(Error::MapChecksumMismatch);
    }

    let listener = W3GSListener::bind().await?;
    let port = listener.port();

    let (map_width, map_height) = map.map.dimension();
    let game_settings = GameSettings::new(
      Default::default(),
      GameSettingsMap {
        path: info.map_path.clone(),
        width: map_width as u16,
        height: map_height as u16,
        sha1: map.checksum.sha1,
        checksum: map.checksum.xoro,
      },
    );
    let worker = Worker {
      ct: ct.clone(),
      map_checksum: map.checksum,
      game_settings,
      rx,
      listener,
      info,
    };

    tokio::spawn(
      async move {
        if let Err(err) = worker.run().await {
          tracing::error!("observer host: {}", err);
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    Ok(Self {
      ct: ct.clone(),
      tx,
      port,
    })
  }
}

impl Drop for ObserverGameHost {
  fn drop(&mut self) {
    self.ct.cancel();
  }
}

enum Cmd {}

struct Worker {
  ct: CancellationToken,
  map_checksum: MapChecksum,
  game_settings: GameSettings,
  rx: Receiver<Cmd>,
  listener: W3GSListener,
  info: ObserverGameInfo,
}

impl Worker {
  async fn run(mut self) -> Result<()> {
    let map_sha1: [u8; 20] = self.map_checksum.sha1;
    let lan_game_info = {
      let mut game_info = flo_lan::GameInfo::new(
        1,
        "FLO-STREAM",
        &self.info.map_path,
        map_sha1,
        self.map_checksum.xoro,
      )?;
      game_info.set_port(self.listener.port());
      game_info
    };

    let _p = MdnsPublisher::start(lan_game_info).await?;
    let slot_info = crate::lan::game::slot::build_player_slot_info(
      SelfPlayer::StreamObserver,
      self.info.random_seed,
      &self.info.slots,
    )?;

    let stream: W3GSStream = loop {
      tokio::select! {
        _ = self.ct.cancelled() => {
          tracing::debug!("cancelled");
          return Ok(());
        },
        res = self.listener.accept() => {
          let mut stream = if let Some(stream) = res? {
            stream
          } else {
            return Ok(())
          };
          if self.handle_lobby(&slot_info, &mut stream).await? {
            break stream;
          }
        }
      }
    };

    Ok(())
  }

  async fn handle_lobby(
    &mut self,
    slot_info: &LanSlotInfo,
    stream: &mut W3GSStream,
  ) -> Result<bool> {
    let total_players = slot_info.slot_info.num_players as usize;
    let mut num_profile = 0;
    let mut num_skins = 0;
    let mut num_unk5 = 0;

    loop {
      tokio::select! {
        _ = self.ct.cancelled() => {
          return Ok(false)
        },
        res = stream.recv() => {
          let pkt: Packet = if let Some(v) = res? {
            v
          } else {
            return Ok(false)
          };

          match pkt.type_id() {
            PacketTypeId::LeaveReq => {
              stream.send(Packet::simple(LeaveAck)?).await?;
              stream.flush().await?;
              return Ok(false)
            },
            PacketTypeId::RejectJoin => {
              let req: ReqJoin = pkt.decode_simple()?;
              self.handle_req_join(slot_info, stream).await?;
            }
            PacketTypeId::MapSize => {
              let payload: MapSize = pkt.decode_simple()?;
              tracing::debug!("<- map size: {:?}", payload);
            }
            PacketTypeId::ProtoBuf => {
              let payload: ProtoBufPayload = pkt.decode_simple()?;
              match payload.type_id {
                ProtoBufMessageTypeId::Unknown2 => {
                  tracing::warn!("-> unexpected protobuf packet type: {:?}", payload.type_id)
                }
                ProtoBufMessageTypeId::PlayerProfile => {
                  num_profile += 1;
                  #[cfg(debug_assertions)]
                  {
                    tracing::debug!(
                      "<-> PlayerProfile: {:?}",
                      payload.decode_message::<PlayerProfileMessage>()?
                    );
                  }
                  stream.send(pkt).await?;
                }
                ProtoBufMessageTypeId::PlayerSkins => {
                  num_skins += 1;
                  stream.send(pkt).await?;
                  #[cfg(debug_assertions)]
                  {
                    tracing::debug!(
                      "<-> PlayerSkins: {:?}",
                      payload.decode_message::<PlayerSkinsMessage>()?
                    );
                  }
                }
                ProtoBufMessageTypeId::PlayerUnknown5 => {
                  num_unk5 += 1;
                  stream.send(pkt).await?;
                  #[cfg(debug_assertions)]
                  {
                    use flo_w3gs::protocol::player::PlayerUnknown5Message;
                    tracing::debug!(
                      "<-> PlayerUnknown5: {:?}",
                      payload.decode_message::<PlayerUnknown5Message>()?
                    );
                  }
                }
                ProtoBufMessageTypeId::UnknownValue(id) => {
                  tracing::warn!("unexpected protobuf packet type id: {}", id)
                }
              }
            }
            PacketTypeId::ChatToHost | PacketTypeId::PongToHost => {},
            _ => {
              tracing::error!("unexpected packet: {:?}", pkt.type_id());
              return Ok(false)
            },
          }
        }
      }

      if num_profile == total_players && num_skins == 1 && num_skins == 1 {
        break;
      }
    }

    stream.send(Packet::simple(CountDownStart)?).await?;
    sleep(Duration::from_secs(1)).await;
    stream.send(Packet::simple(CountDownEnd)?).await?;

    Ok(true)
  }

  async fn handle_req_join(
    &mut self,
    slot_info: &LanSlotInfo,
    stream: &mut W3GSStream,
  ) -> Result<()> {
    let num_players = slot_info.slot_info.num_players as usize;
    let mut replies = Vec::with_capacity((num_players - 1) * 3);

    // slot info
    replies.push(Packet::simple(SlotInfoJoin {
      slot_info: slot_info.slot_info.clone(),
      player_id: slot_info.my_slot_player_id,
      external_addr: SockAddr::from(match stream.local_addr() {
        SocketAddr::V4(addr) => addr,
        SocketAddr::V6(_) => return Err(flo_w3gs::error::Error::Ipv6NotSupported.into()),
      }),
    })?);

    tracing::debug!(
      "-> slot info: slots = {}, players = {}, random_seed = {}",
      slot_info.slot_info.slots().len(),
      slot_info.slot_info.num_players,
      slot_info.slot_info.random_seed
    );

    let mut player_info_packets = Vec::with_capacity(num_players);
    let mut player_skin_packets = Vec::with_capacity(num_players);
    let mut player_profile_packets = Vec::with_capacity(num_players);

    for info in &slot_info.player_infos {
      tracing::debug!(
        "-> PlayerInfo: player: id = {}, name = {}",
        info.slot_player_id,
        info.name
      );
      player_info_packets.push(Packet::simple(PlayerInfo::new(
        info.slot_player_id,
        &info.name,
      ))?);

      tracing::debug!(
        "-> PlayerSkinsMessage: player: id = {}, name = {}",
        info.slot_player_id,
        info.name
      );
      player_skin_packets.push(Packet::simple(ProtoBufPayload::new(PlayerSkinsMessage {
        player_id: info.slot_player_id as u32,
        ..Default::default()
      }))?);

      tracing::debug!(
        "-> PlayerProfileMessage: player: id = {}, name = {}",
        info.slot_player_id,
        info.name
      );
      player_profile_packets.push(Packet::simple(ProtoBufPayload::new(
        PlayerProfileMessage::new(info.slot_player_id, &info.name),
      ))?);
    }

    replies.extend(player_info_packets);
    replies.extend(player_skin_packets);
    replies.extend(player_profile_packets);

    // map check
    replies.push(Packet::simple(MapCheck::new(
      self.map_checksum.file_size as u32,
      self.map_checksum.crc32,
      &self.game_settings,
    ))?);
    tracing::debug!(
      "-> map check: file_size = {}, crc32 = {}",
      self.map_checksum.file_size,
      self.map_checksum.crc32
    );

    stream.send_all(replies).await?;

    Ok(())
  }
}

#[tokio::test]
async fn test_obs_host() {
  use flo_state::Actor;
  let platform = Platform::new(&Default::default()).await.unwrap().start();

  let map_path = r#"maps\W3Champions\v7.1\w3c_Northshire_LV.w3x"#;
  let map = platform
    .send(OpenMap {
      path: map_path.to_string(),
    })
    .await
    .unwrap()
    .unwrap();

  let host = ObserverGameHost::start(
    ObserverGameInfo {
      map_path: map_path.to_string(),
      map_sha1: map.checksum.sha1,
      slots: vec![],
      random_seed: 0,
    },
    platform.addr(),
  )
  .unwrap();
}
