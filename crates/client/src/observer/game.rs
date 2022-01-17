use super::send_queue::SendQueue;
use crate::error::{Error, Result};
use crate::lan::game::slot::{LanSlotInfo, SelfPlayer};
use crate::platform::{GetClientPlatformInfo, OpenMap, Platform};
use flo_lan::MdnsPublisher;
use flo_observer::record::GameRecordData;
use flo_state::Addr;
use flo_types::observer::GameInfo;
use flo_util::binary::SockAddr;
use flo_w3gs::action::IncomingAction;
use flo_w3gs::chat::ChatFromHost;
use flo_w3gs::constants::{PacketTypeId, ProtoBufMessageTypeId};
use flo_w3gs::game::{GameSettings, GameSettingsMap};
use flo_w3gs::lag::{LagPlayer, StartLag, StopLag};
use flo_w3gs::net::{W3GSListener, W3GSStream};
use flo_w3gs::packet::Packet;
use flo_w3gs::protocol::action::OutgoingKeepAlive;
use flo_w3gs::protocol::game::{CountDownEnd, CountDownStart, PlayerLoaded};
use flo_w3gs::protocol::join::{ReqJoin, SlotInfoJoin};
use flo_w3gs::protocol::leave::LeaveAck;
use flo_w3gs::protocol::map::{MapCheck, MapSize};
use flo_w3gs::protocol::packet::ProtoBufPayload;
use flo_w3gs::protocol::player::{PlayerInfo, PlayerProfileMessage, PlayerSkinsMessage};
use flo_w3map::MapChecksum;
use futures::Stream;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::StreamExt;

const DESYNC_GRACE_PERIOD_TICKS: usize = 30;

pub struct ObserverGameHost<S> {
  map_checksum: MapChecksum,
  game_settings: GameSettings,
  listener: W3GSListener,
  info: GameInfo,
  source: S,
}

impl<S> ObserverGameHost<S>
where
  S: Stream<Item = Result<GameRecordData>> + Unpin,
{
  pub async fn new(info: GameInfo, source: S, platform: Addr<Platform>) -> Result<Self> {
    let client_info = platform
      .send(GetClientPlatformInfo::default())
      .await?
      .map_err(|_| Error::War3NotLocated)?;

    if client_info.version != info.game_version {
      return Err(Error::GameVersionMismatch);
    }

    let map = platform
      .send(OpenMap {
        path: info.map.path.clone(),
      })
      .await??;

    if Some(map.checksum.sha1) != info.map.sha1() {
      return Err(Error::MapChecksumMismatch);
    }

    let listener = W3GSListener::bind().await?;

    let (map_width, map_height) = map.map.dimension();
    let game_settings = GameSettings::new(
      Default::default(),
      GameSettingsMap {
        path: info.map.path.clone(),
        width: map_width as u16,
        height: map_height as u16,
        sha1: map.checksum.sha1,
        checksum: map.checksum.xoro,
      },
    );

    Ok(Self {
      map_checksum: map.checksum,
      game_settings,
      listener,
      info,
      source,
    })
  }

  pub async fn play(mut self) -> Result<()> {
    let map_sha1: [u8; 20] = self.map_checksum.sha1;
    let lan_game_info = {
      let mut game_info = flo_lan::GameInfo::new(
        1,
        "FLO-STREAM",
        &self.info.map.path,
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

    let mut stream: W3GSStream = loop {
      tokio::select! {
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

    tracing::debug!("game loop");

    self.play_source(&slot_info, &mut stream).await?;

    Ok(())
  }

  async fn play_source(&mut self, slots: &LanSlotInfo, stream: &mut W3GSStream) -> Result<()> {
    let mut loaded = false;
    let mut tick: u32 = 0;
    let mut agreed_checksums = VecDeque::new();
    let mut pending_checksums = VecDeque::new();
    let mut source_done = false;
    let mut send_queue = SendQueue::new();
    let mut desync_ticks = 0;
    send_queue.set_speed(4.0);

    loop {
      tokio::select! {
        r = self.source.try_next(), if loaded && !source_done => {
          if let Some(r) = r? {
            self.handle_record(r, slots, &mut send_queue, &mut agreed_checksums).await?;
          } else {
            source_done = true;
            send_queue.finish();
          }
        },
        next = send_queue.next() => {
          if let Some(packets) = next {
            stream.send_all(packets.into_iter().map(|(_, p)| p)).await?;
          } else {
            tracing::debug!("source finished");
            break;
          }
        },
        res = stream.recv() => {
          if let Some(pkt) = res? {
            match pkt.type_id() {
              PacketTypeId::OutgoingKeepAlive => {
                let payload: OutgoingKeepAlive = pkt.decode_simple()?;
                let mut checksum = payload.checksum;
                if let Some(pending) = pending_checksums.pop_front() {
                  pending_checksums.push_back(checksum);
                  checksum = pending;
                }
                if let Some(expected) = agreed_checksums.pop_front() {
                  if expected != checksum {
                    let budget = DESYNC_GRACE_PERIOD_TICKS.saturating_sub(desync_ticks);
                    let msg = format!("desync detected: tick = {}, budget = {}, {} != {}", tick, budget, checksum, expected);
                    desync_ticks += 1;
                    if desync_ticks > DESYNC_GRACE_PERIOD_TICKS {
                      tracing::error!("{}", msg);
                      stream.send(Packet::simple(
                        ChatFromHost::private_to_self(slots.my_slot_player_id, msg)
                      )?).await?;
                      break;
                    } else {
                      tracing::warn!("{}", msg);
                    }
                    agreed_checksums.push_front(expected);
                  } else {
                    if desync_ticks > 0 {
                      tracing::debug!("resync: {} ticks", desync_ticks);
                      desync_ticks = 0;
                    }
                  }
                } else {
                  pending_checksums.push_back(checksum);
                }
                tick += 1;
              },
              PacketTypeId::GameLoadedSelf => {
                tracing::debug!("self loaded");
                stream.send(Packet::simple(PlayerLoaded {
                  player_id: slots.my_slot_player_id
                })?).await?;
                loaded = true;
              }
              PacketTypeId::LeaveReq => {
                stream.send(Packet::simple(LeaveAck)?).await?;
                tracing::debug!("leave ack");
                break;
              }
              id => {
                tracing::debug!("recv: {:?}", id)
              }
            }
          } else {
            tracing::debug!("game connection closed");
            break;
          }
        }
      }
    }
    Ok(())
  }

  async fn handle_record(
    &mut self,
    record: GameRecordData,
    slot_info: &LanSlotInfo,
    send_queue: &mut SendQueue,
    checksums: &mut VecDeque<u32>,
  ) -> Result<()> {
    match record {
      GameRecordData::W3GS(pkt) => match pkt.type_id() {
        PacketTypeId::IncomingAction | PacketTypeId::IncomingAction2 => {
          let time_increment_ms =
            IncomingAction::peek_time_increment_ms(pkt.payload.as_ref())? as u64;
          send_queue.push(pkt, time_increment_ms.into());
        }
        PacketTypeId::PlayerLeft => {
          send_queue.push(pkt, None);
        }
        id => {
          tracing::debug!("send: {:?}", id);
        }
      },
      GameRecordData::StartLag(player_ids) => {
        send_queue.push(
          Packet::simple(StartLag::new(
            player_ids
              .into_iter()
              .filter_map(|player_id| {
                Some(LagPlayer {
                  player_id: slot_info
                    .player_infos
                    .iter()
                    .find(|p| p.player_id == player_id)?
                    .slot_player_id,
                  lag_duration_ms: 0,
                })
              })
              .collect(),
          ))?,
          None,
        );
      }
      GameRecordData::StopLag(player_id) => {
        if let Some(slot) = slot_info
          .player_infos
          .iter()
          .find(|p| p.player_id == player_id)
        {
          send_queue.push(
            Packet::simple(StopLag(LagPlayer {
              player_id: slot.slot_player_id,
              lag_duration_ms: 0,
            }))?,
            None,
          );
        }
      }
      GameRecordData::GameEnd => {}
      GameRecordData::TickChecksum { checksum, .. } => {
        checksums.push_back(checksum);
      }
      _ => {}
    }
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
        res = stream.recv() => {
          let pkt: Packet = if let Some(v) = res? {
            v
          } else {
            return Ok(false)
          };

          match dbg!(pkt.type_id()) {
            PacketTypeId::LeaveReq => {
              stream.send(Packet::simple(LeaveAck)?).await?;
              stream.flush().await?;
              return Ok(false)
            },
            PacketTypeId::ReqJoin => {
              let req: ReqJoin = pkt.decode_simple()?;
              self.handle_req_join(req, slot_info, stream).await?;
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

      if num_profile == total_players && num_skins == 1 && num_unk5 == 1 {
        break;
      }
    }

    tracing::debug!("starting game");
    stream.send(Packet::simple(CountDownStart)?).await?;
    sleep(Duration::from_secs(6)).await;
    stream.send(Packet::simple(CountDownEnd)?).await?;
    tracing::debug!("game started");

    let loaded = slot_info
      .player_infos
      .iter()
      .map(|s| {
        Ok(Packet::simple(PlayerLoaded {
          player_id: s.slot_player_id,
        })?)
      })
      .collect::<Result<Vec<_>>>()?;

    stream.send_all(loaded).await?;

    Ok(true)
  }

  async fn handle_req_join(
    &mut self,
    req: ReqJoin,
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

    let obs_player_id = slot_info.my_slot_player_id;
    let obs_name = req.player_name.to_string_lossy();
    player_profile_packets.push(Packet::simple(ProtoBufPayload::new(
      PlayerProfileMessage::new(obs_player_id, &obs_name),
    ))?);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_reader() {
  use flo_observer::record::GameRecordData;
  use flo_observer_fs::GameDataArchiveReader;
  use flo_w3gs::action::IncomingAction;
  use flo_w3gs::constants::PacketTypeId;
  let r = GameDataArchiveReader::open(flo_util::sample_path!("replay", "703450.gz"))
    .await
    .unwrap();
  let records = r.records().collect_vec().await.unwrap();
  println!("records = {}", records.len());
  let mut max_len = 0;
  let mut time = 0;
  for record in records {
    match record {
      GameRecordData::W3GS(pkt) => {
        if pkt.type_id() == PacketTypeId::IncomingAction {
          let payload: IncomingAction = pkt.decode_payload().unwrap();
          max_len = std::cmp::max(max_len, pkt.payload.len());
          time += payload.0.time_increment_ms as u32;
          if pkt.payload.len() > 5000 {
            use std::collections::BTreeMap;
            let mut map = BTreeMap::new();
            for action in &payload.0.actions {
              (*map.entry(action.player_id).or_insert_with(|| 0)) += action.data.len();
            }
            dbg!(map);
          }
          println!(
            "len = {}, time = {:?}, tdiff = {}, n = {}",
            pkt.payload.len(),
            Duration::from_millis(time as _),
            payload.0.time_increment_ms,
            payload.0.actions.len()
          );
        }
      }
      _ => {}
    }
  }
  println!("max_len = {}", max_len);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stream() -> crate::error::Result<()> {
  use flo_state::Actor;
  dotenv::dotenv().unwrap();
  flo_log_subscriber::init_env_override("flo_client");

  let game_id = 2118777;
  let token = flo_observer::token::create_observer_token(game_id, None).unwrap();
  let (i, s) = crate::observer::source::NetworkSource::connect("127.0.0.1:3557", token).await?;
  // let s = crate::observer::source::ArchiveFileSource::load(format!("/Users/fluxxu/Downloads/{}", game_id)).await?;

  // tracing::info!("game: {:#?}", i);

  let platform = Platform::new(&Default::default()).await.unwrap().start();

  let host = ObserverGameHost::new(i, s, platform.addr()).await?;
  host.play().await?;

  Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_archive() -> crate::error::Result<()> {
  pub use flo_grpc::controller::flo_controller_client::FloControllerClient;
  use flo_grpc::Channel;
  use flo_state::Actor;
  use s2_grpc_utils::S2ProtoUnpack;
  use tonic::service::{interceptor::InterceptedService, Interceptor};

  pub async fn get_grpc_client() -> FloControllerClient<InterceptedService<Channel, WithSecret>> {
    let host = std::env::var("CONTROLLER_HOST").unwrap().clone();
    let channel = Channel::from_shared(format!("tcp://{}:3549", host))
      .unwrap()
      .connect()
      .await
      .unwrap();
    FloControllerClient::with_interceptor(channel, WithSecret)
  }

  #[derive(Clone)]
  pub struct WithSecret;

  impl Interceptor for WithSecret {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
      req.metadata_mut().insert(
        "x-flo-secret",
        std::env::var("CONTROLLER_SECRET").unwrap().parse().unwrap(),
      );
      Ok(req)
    }
  }

  dotenv::dotenv().unwrap();
  flo_log_subscriber::init_env_override("flo_client");

  let game_id = 2114757;
  let s = crate::observer::source::ArchiveFileSource::load(dbg!(format!(
    "../../target/games/{}.gz",
    game_id
  )))
  .await?;
  let mut ctrl = get_grpc_client().await;
  let game = GameInfo::unpack(
    ctrl
      .get_game(flo_grpc::controller::GetGameRequest { game_id })
      .await
      .unwrap()
      .into_inner()
      .game
      .unwrap(),
  )
  .unwrap();

  // tracing::info!("game: {:#?}", i);

  let platform = Platform::new(&Default::default()).await.unwrap().start();

  let host = ObserverGameHost::new(game, s, platform.addr()).await?;
  host.play().await?;

  Ok(())
}
