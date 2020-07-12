use futures::sink::SinkExt;
use futures::stream::{StreamExt, TryStreamExt};
use std::collections::HashMap;
use std::ffi::CString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::delay_for;
use tracing_futures::Instrument;

use flo_lan::{GameInfo, MdnsPublisher};
use flo_util::binary::*;
use flo_w3gs::game::GameSettings;
use flo_w3gs::net::W3GSStream;
use flo_w3gs::packet::{Packet, PacketPayload, PacketProtoBufMessage};
use flo_w3gs::protocol::action::*;
use flo_w3gs::protocol::constants::PacketTypeId::{IncomingAction, IncomingAction2, PlayerLoaded};
use flo_w3map::{MapChecksum, W3Map};
use flo_w3storage::W3Storage;

#[tokio::main]
async fn main() {
  flo_log::init_env("poc_host=debug");

  use flo_w3gs::net::W3GSListener;

  let mut listener = W3GSListener::bind().await.unwrap();
  tracing::info!("listening on {}", listener.local_addr());
  let port = listener.port();

  let mut game_info =
    GameInfo::decode_bytes(&flo_util::sample_bytes!("lan", "mdns_gamedata.bin")).unwrap();
  game_info.name = CString::new("DEMO").unwrap();
  game_info.data.name = CString::new("DEMO").unwrap();
  game_info.data.port = port;
  game_info.create_time = std::time::SystemTime::now();
  game_info.game_id = "1".to_owned();

  let map_path = game_info.data.settings.map_path.to_str().unwrap();
  tracing::debug!("map: {}", map_path);

  let storage = W3Storage::from_env().unwrap();
  let (map, checksum) = W3Map::open_storage_with_checksum(&storage, map_path).unwrap();
  let map = Arc::new(map);

  // game_info.data.settings.map_xoro = checksum.xoro;

  let _p = MdnsPublisher::start(game_info.clone()).await.unwrap();
  while let Some(stream) = listener.incoming().try_next().await.unwrap() {
    tracing::debug!("connected: {}", stream.local_addr());

    let addr = stream.local_addr();
    let peer_addr = stream.peer_addr();
    let map = map.clone();
    let game_info = game_info.clone();
    let checksum = checksum.clone();
    tokio::spawn(async move {
      run_lobby(
        peer_addr.clone(),
        stream,
        &game_info.data.settings,
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
  #[error("invalid player id")]
  InvalidPlayerId,
}

async fn run_lobby(
  server_addr: SocketAddr,
  mut transport: W3GSStream,
  settings: &GameSettings,
  map: &W3Map,
  map_checksum: &MapChecksum,
) -> Result<(), LobbyError> {
  use flo_w3gs::protocol::player::PlayerProfileMessage;
  use flo_w3gs::protocol::slot::SlotInfo;
  use std::time::Instant;
  let mut slot_info = SlotInfo::new(2, 2);
  let slot = slot_info.join().unwrap();
  let time = Instant::now();

  slot.color = 23;
  slot.download_status = 100;

  let mut profiles = HashMap::new();
  profiles.insert(
    slot.player_id,
    PlayerProfileMessage::new(slot.player_id, "Flo Host"),
  );

  // <- ReqJoin
  let req = {
    use flo_w3gs::protocol::join::ReqJoin;
    let packet: Packet = transport.recv().await?;
    let req_join: ReqJoin = packet.decode_simple_payload()?;
    tracing::debug!("player request to join: {:?}", req_join.player_name);
    req_join
  };

  // -> SlotInfoJoin
  let player_id = {
    use flo_w3gs::protocol::join::{RejectJoin, SlotInfoJoin};

    if let Some(slot) = slot_info.join() {
      slot.team = 1;
      slot.color = 1;
      let player_id = slot.player_id;
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
    } else {
      transport
        .send(Packet::with_simple_payload(RejectJoin::FULL)?)
        .await?;
      tracing::debug!("rejected: full");
      return Ok(());
    }
  };

  // -> PlayerInfo (Host)
  {
    use flo_w3gs::protocol::player::PlayerInfo;
    let payload = PlayerInfo::new(1, CString::new("HOST").unwrap());
    transport
      .send(Packet::with_simple_payload(payload)?)
      .await?;
    tracing::debug!("player info sent");
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

    if payload.player_id > 23
      || slot_info
        .find_active_player_slot_mut(payload.player_id as u8)
        .is_none()
    {
      return Err(LobbyError::InvalidPlayerId);
    }

    profiles.insert(payload.player_id as u8, payload);

    transport.send(packet).await?;
    tracing::debug!("PlayerProfile ack sent");
  }

  // #14:
  // -> PlayerSkin (Host)
  {
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::player::PlayerSkinsMessage;

    let payload = ProtoBufPayload::new(PlayerSkinsMessage {
      player_id: 1,
      ..Default::default()
    });
    transport
      .send(Packet::with_simple_payload(payload)?)
      .await?;
    tracing::debug!("PlayerSkins (Host)  sent");
  }

  // #15/#17:
  // -> SlotInfo
  {
    slot_info
      .find_active_player_slot_mut(1)
      .unwrap()
      .download_status = 100;

    let packet = Packet::with_simple_payload(slot_info.clone())?;
    transport.send(packet).await?;
    tracing::debug!("SlotInfo sent");
  }

  // #16:
  // -> PlayerProfile (Host)
  {
    use flo_w3gs::protocol::packet::ProtoBufPayload;
    use flo_w3gs::protocol::player::PlayerProfileMessage;

    let packet =
      Packet::with_simple_payload(ProtoBufPayload::new(profiles.get(&1).cloned().unwrap()))?;

    transport.send(packet).await?;
    tracing::debug!("Host PlayerProfile sent");
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

  {
    use flo_w3gs::game::*;
    transport
      .send(Packet::with_simple_payload(CountDownStart)?)
      .await?;
    tracing::debug!("count down start sent");

    delay_for(Duration::from_secs(1)).await;

    transport
      .send(Packet::with_simple_payload(CountDownEnd)?)
      .await?;
    tracing::debug!("count down end sent");
  }

  // #21
  // -> PlayerLoaded (Host)
  {
    use flo_w3gs::protocol::player::PlayerLoaded;
    let packet = Packet::with_simple_payload(PlayerLoaded::new(1))?;
    transport.send(packet).await?;
    tracing::debug!("PlayerLoaded (Host) sent");
  }

  use flo_w3gs::error::Error;
  use std::sync::Mutex;
  use tokio::sync::mpsc;
  let (tx, mut rx) = mpsc::channel(100);
  let action_q: Arc<Mutex<Vec<Bytes>>> = Arc::new(Mutex::new(vec![]));

  tokio::spawn(
    {
      let mut sender = tx.clone();
      let q = action_q.clone();
      async move {
        use flo_w3gs::action::*;
        use tokio::time::{delay_until, Instant};
        let mut t = Instant::now();
        loop {
          delay_until(t + Duration::from_millis(25)).await;
          let now = Instant::now();
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

              let p = IncomingAction2(TimeSlot {
                time_increment_ms: (d.subsec_millis() as u16 + d.as_secs() as u16) * 4,
                actions: vec![PlayerAction {
                  player_id: 2,
                  data: last,
                }],
              });
              sender.send(Packet::with_payload(p).unwrap()).await.unwrap();
            } else {
              let p = IncomingAction(TimeSlot {
                time_increment_ms: (d.subsec_millis() as u16 + d.as_secs() as u16) * 4,
                actions: items
                  .into_iter()
                  .map(|data| PlayerAction { player_id: 2, data })
                  .collect(),
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
          PacketTypeId::OutgoingAction => {
            let req: OutgoingAction = p.decode_payload()?;
            // tracing::debug!("action");
            action_q.lock().unwrap().push(req.data);
          }
          PacketTypeId::OutgoingKeepAlive => {
            // tracing::debug!("action ack");
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
