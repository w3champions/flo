use futures::stream::TryStreamExt;
use std::collections::HashMap;
use std::ffi::CString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::{sleep, delay_until, Instant};
use tracing_futures::Instrument;

use flo_lan::{GameInfo, MdnsPublisher};
use flo_util::binary::*;
use flo_w3gs::game::GameSettings;
use flo_w3gs::net::W3GSStream;
use flo_w3gs::packet::{Packet, PacketPayload, PacketProtoBufMessage};
use flo_w3gs::protocol::action::*;
use flo_w3gs::protocol::constants::PacketTypeId::{IncomingAction, IncomingAction2, PlayerLoaded};
use flo_w3map::{MapChecksum, W3Map};
use flo_w3replay::{Record, ReplayInfo, W3Replay};
use flo_w3storage::W3Storage;

#[tokio::main]
async fn main() {
  flo_log::init_env("replay2=debug");

  use flo_w3gs::net::W3GSListener;

  let mut listener = W3GSListener::bind().await.unwrap();
  tracing::info!("listening on {}", listener.local_addr());
  let port = listener.port();

  let mut game_info = GameInfo::from_replay(flo_util::sample_path!("replay", "bn.w3g")).unwrap();
  game_info.set_port(port);
  game_info.data.settings.map_checksum = 3389385288;

  dbg!(&game_info);

  let (inspect, rest) = W3Replay::inspect(flo_util::sample_path!("replay", "bn.w3g")).unwrap();
  let inspect = Arc::new(inspect);

  // dbg!(&inspect);
  //0x6c744e17 != 0x6c75722f

  let rest: Arc<Vec<_>> = Arc::new(rest.map(|r| r.unwrap()).collect());

  let storage = W3Storage::from_env().unwrap();
  let (map, checksum) =
    W3Map::open_storage_with_checksum(&storage, game_info.data.settings.map_path.to_str().unwrap())
      .unwrap();
  let map = Arc::new(map);

  let _p = MdnsPublisher::start(game_info.clone()).await.unwrap();
  while let Some(stream) = listener.incoming().try_next().await.unwrap() {
    tracing::debug!("connected: {}", stream.local_addr());

    let addr = stream.local_addr();
    let peer_addr = stream.peer_addr();
    let map = map.clone();
    let game_info = game_info.clone();
    let checksum = checksum.clone();
    let inspect = inspect.clone();
    let rest = rest.clone();
    tokio::spawn(async move {
      run_lobby(
        peer_addr.clone(),
        stream,
        &game_info.data.settings,
        &inspect,
        rest,
        &map,
        &checksum,
      )
      .await
      .map_err(|e| tracing::error!("{}", e))
      .ok();
      tracing::debug!("dropped: {}", addr);
    });
  }
}

#[derive(thiserror::Error, Debug)]
enum LobbyError {
  #[error(transparent)]
  Protocol(#[from] flo_w3gs::error::Error),
}

async fn run_lobby(
  server_addr: SocketAddr,
  mut transport: W3GSStream,
  settings: &GameSettings,
  inspect: &ReplayInfo,
  rest: Arc<Vec<Record>>,
  map: &W3Map,
  map_checksum: &MapChecksum,
) -> Result<(), LobbyError> {
  use flo_w3gs::protocol::player::PlayerProfileMessage;
  use flo_w3gs::protocol::slot::SlotInfo;
  use std::time::Instant;
  let slot_info = inspect.slots.clone();
  let time = Instant::now();

  // find first ob player
  let player_id = inspect
    .slots
    .slots()
    .iter()
    .find(|s| s.team == 24)
    .unwrap()
    .player_id;

  let mut players: Vec<_> = inspect
    .players
    .iter()
    .filter(|p| p.id != player_id)
    .collect();
  players.push(&inspect.game.host_player_info);

  let mut profiles = HashMap::new();

  profiles.insert(
    inspect.game.host_player_info.id,
    PlayerProfileMessage::new(
      inspect.game.host_player_info.id,
      inspect.game.host_player_info.name.to_str().unwrap(),
    ),
  );

  for player in &inspect.players {
    profiles.insert(
      player.id,
      PlayerProfileMessage::new(player.id, player.name.to_str().unwrap()),
    );
  }

  // <- ReqJoin
  let _req = {
    use flo_w3gs::protocol::join::ReqJoin;
    let packet: Packet = transport.recv().await?;
    let req_join: ReqJoin = packet.decode_simple_payload()?;
    tracing::debug!("player request to join: {:?}", req_join.player_name);
    req_join
  };

  // -> SlotInfoJoin
  {
    use flo_w3gs::protocol::join::{RejectJoin, SlotInfoJoin};

    let payload = SlotInfoJoin {
      slot_info: slot_info.clone(),
      player_id,
      external_addr: SockAddr::from(match server_addr {
        SocketAddr::V4(addr) => addr,
        SocketAddr::V6(_) => return Err(flo_w3gs::error::Error::Ipv6NotSupported.into()),
      }),
    };

    transport
      .send(Packet::with_simple_payload(payload)?)
      .await?;
    tracing::debug!("accepted: player_id = {}", player_id);
    player_id
  };

  for player in &players {
    // -> PlayerInfo
    {
      use flo_w3gs::protocol::player::PlayerInfo;
      let payload = PlayerInfo::new(player.id, player.name.clone());
      transport
        .send(Packet::with_simple_payload(payload)?)
        .await?;
      tracing::debug!("player info sent: {}", player.id);
    }
  }

  // -> MapCheck
  {
    use flo_w3gs::protocol::map::MapCheck;
    let payload = MapCheck::new(map.file_size() as u32, map_checksum.crc32, settings);
    transport
      .send(Packet::with_simple_payload(payload)?)
      .await?;
    tracing::debug!("map check sent");
  }

  // <- MapSize
  {
    use flo_w3gs::protocol::map::MapSize;
    let packet: Packet = transport.recv().await?;
    let payload: MapSize = packet.decode_simple_payload()?;
    tracing::debug!("MapSize received: {:?}", payload);
  }

  // <- Skins
  // -> Skins
  {
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::player::PlayerSkinsMessage;
    let packet: Packet = transport.recv().await?;
    let payload: PlayerSkinsMessage = packet.decode_protobuf()?;
    tracing::debug!("PlayerSkins received: {:?}", payload);

    transport.send(packet).await?;
    tracing::debug!("PlayerSkins ack sent");
  }

  // <- PlayerUnknown5Message
  // -> PlayerUnknown5Message
  {
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::player::PlayerUnknown5Message;
    let packet: Packet = transport.recv().await?;
    let payload: PlayerUnknown5Message = packet.decode_protobuf()?;
    tracing::debug!("PlayerUnknown5Message received: {:?}", payload);

    transport.send(packet).await?;
    tracing::debug!("PlayerUnknown5Message ack sent");
  }

  // <- PlayerProfile
  // -> PlayerProfile
  {
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::player::PlayerProfileMessage;
    let packet: Packet = transport.recv().await?;
    let payload: PlayerProfileMessage = packet.decode_protobuf()?;
    tracing::debug!("PlayerProfileMessage received: {:?}", payload);

    transport.send(packet).await?;
    tracing::debug!("PlayerProfile ack sent");
  }

  for player in &players {
    // #14:
    // -> PlayerSkin
    {
      use flo_w3gs::protocol::packet::ProtoBufPayload;
      use flo_w3gs::protocol::player::PlayerSkinsMessage;

      let payload = ProtoBufPayload::new(PlayerSkinsMessage {
        player_id: player.id as u32,
        ..Default::default()
      });
      transport
        .send(Packet::with_simple_payload(payload)?)
        .await?;
      tracing::debug!("PlayerSkins sent: {}", player.id);
    }
  }

  // #15/#17:
  // -> SlotInfo
  {
    let packet = Packet::with_simple_payload(slot_info.clone())?;
    transport.send(packet).await?;
    tracing::debug!("SlotInfo sent");
  }

  // #16:
  // -> PlayerProfile all
  for player in &players {
    {
      use flo_w3gs::protocol::packet::ProtoBufPayload;
      use flo_w3gs::protocol::player::PlayerProfileMessage;

      let packet = Packet::with_simple_payload(ProtoBufPayload::new(
        profiles.get(&player.id).cloned().unwrap(),
      ))?;

      transport.send(packet).await?;
      tracing::debug!("PlayerProfile sent: {}", player.id);
    }
  }

  // #18
  // -> PingFromHost
  {
    use flo_w3gs::protocol::ping::PingFromHost;
    let packet = Packet::with_simple_payload(PingFromHost::payload_since(time))?;
    transport.send(packet).await?;
    tracing::debug!("PingFromHost sent");
  }

  loop {
    use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::ping::PongToHost;

    let p = transport.recv().await?;
    match p.type_id() {
      PongToHost::PACKET_TYPE_ID => {
        let req: PongToHost = p.decode_simple_payload()?;
        tracing::debug!("PongToHost: {:?}", req);
      }
      LeaveReq::PACKET_TYPE_ID => {
        let req: LeaveReq = p.decode_simple_payload()?;
        tracing::debug!("LeaveReq: {:?}", req);
        transport
          .send(Packet::with_simple_payload(LeaveAck)?)
          .await?;
        return Ok(());
      }
      ProtoBufPayload::PACKET_TYPE_ID => {
        let req: ProtoBufPayload = p.decode_simple_payload()?;
        tracing::debug!("ProtoBufPayload: {:?}", req);
        match req.message_type_id() {
          PlayerProfileMessage::MESSAGE_TYPE_ID => {
            let msg: PlayerProfileMessage = req.decode_message()?;
            tracing::debug!("PlayerProfileMessage: {:?}", msg);
            break;
          }
          _ => {
            tracing::debug!("recv: {:?}", req);
          }
        }
      }
      _ => {
        tracing::debug!("recv: {:?}", p);
      }
    }
  }

  // sleep(Duration::from_secs(100)).await;

  {
    use flo_w3gs::game::*;
    transport
      .send(Packet::with_simple_payload(CountDownStart)?)
      .await?;
    tracing::debug!("count down start sent");

    sleep(Duration::from_secs(1)).await;

    transport
      .send(Packet::with_simple_payload(CountDownEnd)?)
      .await?;
    tracing::debug!("count down end sent");
  }

  for player in &players {
    // #21
    // -> PlayerLoaded
    {
      use flo_w3gs::protocol::player::PlayerLoaded;
      let packet = Packet::with_simple_payload(PlayerLoaded::new(player.id))?;
      transport.send(packet).await?;
      tracing::debug!("PlayerLoaded sent: {}", player.id);
    }
  }

  use flo_w3gs::error::Error;
  use std::sync::atomic::AtomicU32;
  use std::sync::atomic::Ordering;
  use std::sync::Mutex;
  use tokio::sync::mpsc;
  let (tx, mut rx) = mpsc::channel(5);
  let action_q: Arc<Mutex<Vec<Bytes>>> = Arc::new(Mutex::new(vec![]));
  let mut all_players: Vec<_> = inspect.players.iter().map(|p| p.id).collect();
  all_players.push(inspect.game.host_player_info.id);
  let loaded = Arc::new(Notify::new());
  let ack = Arc::new(Notify::new());
  let checksum = Arc::new(AtomicU32::new(0));

  tokio::spawn(
    {
      let mut sender = tx.clone();
      let q = action_q.clone();
      let rest = rest.clone();
      let loaded = loaded.clone();
      let _ack = ack.clone();
      let checksum = checksum.clone();
      async move {
        use flo_w3gs::action::*;
        let mut t = tokio::time::Instant::now();
        let mut ms = 0;

        loaded.notified().await;

        let mut batched_slot = TimeSlot {
          time_increment_ms: 0,
          actions: vec![],
        };

        for (_i, record) in rest.iter().enumerate() {
          match *record {
            Record::ChatMessage(ref msg) => {
              use flo_w3gs::chat::*;
              let p = ChatFromHost::new(ChatToHost {
                to_players_len: all_players.len() as u8,
                to_players: all_players.clone(),
                from_player: msg.player_id,
                message: msg.message.clone(),
              });
              sender
                .send(Packet::with_simple_payload(p).unwrap())
                .await
                .unwrap();
            }
            Record::TimeSlotFragment(ref f) => {
              tracing::debug!(
                "TimeSlotFragment: ms = {}, actions = {}",
                f.0.time_increment_ms,
                f.0.actions.len()
              );
              unreachable!()
              // if f.0.time_increment_ms > 0 {
              //   ms = ms + (f.0.time_increment_ms as u64);
              //   delay_until(t + Duration::from_millis(ms)).await;
              // }
              // sender
              //   .send(
              //     Packet::with_payload(IncomingAction2(TimeSlot {
              //       time_increment_ms: f.0.time_increment_ms,
              //       actions: f.0.actions.clone(),
              //     }))
              //     .unwrap(),
              //   )
              //   .await
              //   .unwrap();
            }
            Record::TimeSlot(ref f) => {
              // tracing::debug!(
              //   "TimeSlot: ms = {}, actions = {}",
              //   f.time_increment_ms,
              //   f.actions.len()
              // );

              batched_slot.time_increment_ms = batched_slot.time_increment_ms + f.time_increment_ms;
              batched_slot.actions.extend(f.actions.clone());

              // if f.time_increment_ms > 0 {
              //   ms = ms + (f.time_increment_ms as u64);
              //   delay_until(t + Duration::from_millis(ms)).await;
              // }
              // sender
              //   .send(
              //     Packet::with_payload(IncomingAction(TimeSlot {
              //       time_increment_ms: f.time_increment_ms,
              //       actions: f.actions.clone(),
              //     }))
              //     .unwrap(),
              //   )
              //   .await
              //   .unwrap();
            }
            Record::TimeSlotAck(ref p) => {
              if batched_slot.time_increment_ms >= 500 {
                tracing::debug!(
                  "batched actions: time = {}, len = {}",
                  batched_slot.time_increment_ms,
                  batched_slot.actions.len()
                );

                ms = ms + (batched_slot.time_increment_ms as u64);
                delay_until(t + Duration::from_millis(ms / 10)).await;

                if batched_slot
                  .actions
                  .iter()
                  .map(|a| a.data.len())
                  .sum::<usize>()
                  > 1452
                {
                  let mut fragments = vec![];

                  let mut batch_size = 0;
                  let mut batch = vec![];
                  for action in std::mem::replace(&mut batched_slot.actions, vec![]) {
                    if batch_size + action.byte_len() > 1452 {
                      fragments.push(TimeSlot {
                        time_increment_ms: 0,
                        actions: std::mem::replace(&mut batch, vec![]),
                      });
                      batch_size = 0;
                    }
                    batch_size = batch_size + action.byte_len();
                    batch.push(action);
                  }

                  tracing::debug!("sending multiple action packets: {}", fragments.len() + 1);

                  for fragment in fragments {
                    sender
                      .send(Packet::with_payload(IncomingAction2(fragment)).unwrap())
                      .await
                      .unwrap();
                  }

                  sender
                    .send(
                      Packet::with_payload(IncomingAction(TimeSlot {
                        time_increment_ms: batched_slot.time_increment_ms,
                        actions: batch,
                      }))
                      .unwrap(),
                    )
                    .await
                    .unwrap();
                } else {
                  sender
                    .send(
                      Packet::with_payload(IncomingAction(TimeSlot {
                        time_increment_ms: batched_slot.time_increment_ms,
                        actions: std::mem::replace(&mut batched_slot.actions, vec![]),
                      }))
                      .unwrap(),
                    )
                    .await
                    .unwrap();
                }

                batched_slot.time_increment_ms = 0;
                checksum.store(p.checksum, Ordering::SeqCst);

                // ack.notified().await;
              }
            }
            ref r => {
              tracing::debug!("{:?}", r);
            }
          }
        }

        tracing::info!("replay actions done.");

        loop {
          delay_until(t + Duration::from_millis(100)).await;
          let now = tokio::time::Instant::now();
          let d = now - t;
          t = now;

          let items = {
            let mut lock = q.lock().unwrap();
            if lock.len() > 0 {
              Some(std::mem::replace(&mut lock as &mut Vec<_>, vec![]))
            } else {
              None
            }
          };

          if let Some(mut items) = items {
            if items.len() > 1 {
              tracing::info!("batch actions: {}", items.len());

              let last = items.pop().unwrap();
              let batched = items
                .into_iter()
                .map(|item| PlayerAction {
                  player_id: 2,
                  data: item,
                })
                .collect();

              let batch = TimeSlot {
                time_increment_ms: 0,
                actions: batched,
              };
              for batch in batch.split_chunks() {
                sender
                  .send(Packet::with_payload(IncomingAction2(batch)).unwrap())
                  .await
                  .unwrap();
              }

              let p = IncomingAction(TimeSlot {
                time_increment_ms: (d.subsec_millis() as u16 + d.as_secs() as u16),
                actions: vec![PlayerAction {
                  player_id: 2,
                  data: last,
                }],
              });
              sender.send(Packet::with_payload(p).unwrap()).await.unwrap();
            } else {
              let p = IncomingAction(TimeSlot {
                time_increment_ms: (d.subsec_millis() as u16 + d.as_secs() as u16),
                // action: items.pop().map(|data| PlayerAction { player_id: 2, data }),
                actions: vec![],
              });
              sender.send(Packet::with_payload(p).unwrap()).await.unwrap();
            }
          } else {
            let p = IncomingAction(TimeSlot {
              time_increment_ms: (d.subsec_millis() as u16 + d.as_secs() as u16),
              actions: vec![],
            });
            sender.send(Packet::with_payload(p).unwrap()).await.unwrap();
          }
        }
      }
    }
    .instrument(tracing::debug_span!("action loop")),
  );

  loop {
    use flo_w3gs::chat::*;
    use flo_w3gs::game::GameLoadedSelf;
    use flo_w3gs::protocol::constants::PacketTypeId;
    use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::ping::PongToHost;
    use flo_w3gs::protocol::player::PlayerLoaded;

    tokio::select! {
      r = transport.recv() => {
        let p = r?;
        match p.type_id() {
          LeaveReq::PACKET_TYPE_ID => {
            let req: LeaveReq = p.decode_simple_payload()?;
            tracing::debug!("LeaveReq: {:?}", req);
            transport
              .send(Packet::with_simple_payload(LeaveAck)?)
              .await?;
            return Ok(());
          }
          GameLoadedSelf::PACKET_TYPE_ID => {
            tracing::debug!("Player {} loaded", player_id);
            // #5139
            transport
              .send(Packet::with_simple_payload(PlayerLoaded::new(player_id))?)
              .await?;
            tracing::debug!("Player {} loaded ack sent", player_id);
            // return Ok(());

            loaded.notify();
          }
          ProtoBufPayload::PACKET_TYPE_ID => {
            let req: ProtoBufPayload = p.decode_simple_payload()?;
            tracing::debug!("ProtoBufPayload: {:?}", req);
            match req.message_type_id() {
              PlayerProfileMessage::MESSAGE_TYPE_ID => {
                let msg: PlayerProfileMessage = req.decode_message()?;
                tracing::debug!("PlayerProfileMessage: {:?}", msg);
                continue;
              }
              _ => {
                tracing::debug!("recv: {:?}", req);
              }
            }
          }
          PacketTypeId::OutgoingAction => {
            let req: OutgoingAction = p.decode_payload()?;
            // tracing::debug!("action");
            action_q.lock().unwrap().push(req.data);
          }
          PacketTypeId::OutgoingKeepAlive => {
            let _req: OutgoingKeepAlive = p.decode_simple_payload()?;
            let _checksum = checksum.load(Ordering::SeqCst);
            // tracing::debug!("ack: {:?}", req);
            // if checksum != req.checksum {
            //   tracing::error!("desync: 0x{:x} != 0x{:x}", req.checksum, checksum);
            //   break;
            // } else {
            //   tracing::info!("checksum ok");
            // }
            // ack.notify();
          }
          PacketTypeId::ChatToHost => {
            let mut req: ChatToHost = p.decode_simple_payload()?;
            tracing::debug!("ChatToHost: {:?}", req);
            req.to_players[0] = 2;
            req.from_player = 1;
            transport
              .send(Packet::with_simple_payload(ChatFromHost::new(req))?)
              .await?;
          }
          _ => {
            tracing::debug!("recv: {:?}", p);
          }
        }
      }
      action = rx.recv() => {
        if let Some(action) = action {
          transport.send(action).await?;
          // tracing::debug!("action sent");
        } else {
          tracing::debug!("action rx broken");
          break;
        }
      }
    }
  }

  Ok(())
}
