use std::net::SocketAddr;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_event::EventSender;
use flo_util::binary::SockAddr;
use flo_w3gs::net::{W3GSListener, W3GSStream};
use flo_w3gs::protocol::chat::{ChatFromHost, ChatToHost};
use flo_w3gs::protocol::join::{RejectJoin, ReqJoin, SlotInfoJoin};
use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};
use flo_w3gs::protocol::map::{MapCheck, MapSize};
use flo_w3gs::protocol::packet::*;
use flo_w3gs::protocol::player::{
  PlayerInfo, PlayerProfileMessage, PlayerSkinsMessage, PlayerUnknown5Message,
};
use flo_w3map::MapChecksum;

use crate::controller::LocalGameInfo;
use crate::error::*;
use crate::lan::game::LanGameInfo;
use crate::lan::LanEvent;
use flo_w3gs::protocol::constants::ProtoBufMessageTypeId;

#[derive(Debug)]
pub struct LanGameListener {
  port: u16,
  state: Arc<State>,
}

impl LanGameListener {
  pub async fn bind(info: LanGameInfo, event_sender: EventSender<LanEvent>) -> Result<Self> {
    let listener = W3GSListener::bind().await?;
    let port = listener.port();

    tracing::debug!("listening on port {}", port);

    let state = Arc::new(State {
      dropper: Arc::new(Notify::new()),
      info,
      event_sender,
    });

    tokio::spawn(
      state
        .clone()
        .serve(listener)
        .instrument(tracing::debug_span!("worker")),
    );

    Ok(LanGameListener { port, state })
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

impl Drop for LanGameListener {
  fn drop(&mut self) {
    self.state.dropper.notify();
  }
}

#[derive(Debug)]
struct State {
  dropper: Arc<Notify>,
  info: LanGameInfo,
  event_sender: EventSender<LanEvent>,
}

impl State {
  async fn serve(self: Arc<Self>, mut listener: W3GSListener) {
    'listener: loop {
      let dropped = self.dropper.notified();
      let mut incoming = listener.incoming();

      tokio::pin!(dropped);

      let next = tokio::select! {
        _ = &mut dropped => {
          break;
        }
        next = incoming.try_next() => {
          next
        }
      };

      tracing::debug!("connected");

      let mut stream = match next {
        Ok(Some(stream)) => stream,
        Ok(None) => break,
        Err(err) => {
          tracing::error!("lan stream: {}", err);
          continue;
        }
      };

      let mut join = JoinHandler::new(&self.info, &mut stream);

      tokio::select! {
        _ = &mut dropped => {
          break;
        }
        res = join.run() => {
          match res {
            Ok(joined) => {
              if joined {
                tracing::debug!("joined");
              } else {
                tracing::debug!("left");
                continue;
              }
            }
            Err(err) => {
              tracing::error!("join: {}", err);
              continue;
            }
          }
        }
      }

      if let Err(_) = self
        .event_sender
        .clone()
        .send(LanEvent::PlayerJoinEvent)
        .await
      {
        break;
      }
    }

    tracing::debug!("exiting");
  }
}

#[derive(Debug)]
struct JoinHandler<'a> {
  info: &'a LanGameInfo,
  stream: &'a mut W3GSStream,
}

impl<'a> JoinHandler<'a> {
  fn new(info: &'a LanGameInfo, stream: &'a mut W3GSStream) -> Self {
    Self { info, stream }
  }

  async fn run(&mut self) -> Result<bool> {
    loop {
      tokio::select! {
        next = self.stream.recv() => {
          let pkt = next?;
          if let Some(pkt) = pkt {
            if pkt.type_id() == LeaveReq::PACKET_TYPE_ID {
              self.stream.send(Packet::simple(LeaveAck)?).await?;
              self.stream.flush().await?;
              return Ok(false)
            }

            self.handle_packet(pkt).await?;
          } else {
            return Err(Error::StreamClosed)
          }
        }
      }
    }
    Ok(true)
  }

  async fn handle_packet(&mut self, pkt: Packet) -> Result<()> {
    let &LanGameInfo {
      ref game,
      ref slot_info,
      ref map_checksum,
      ref game_settings,
    } = self.info;

    match pkt.type_id() {
      ReqJoin::PACKET_TYPE_ID => {
        let num_players = slot_info.player_infos.len();
        let mut replies = Vec::with_capacity(num_players * 3);

        // slot info
        replies.push(Packet::simple(SlotInfoJoin {
          slot_info: slot_info.slot_info.clone(),
          player_id: slot_info.slot_player_id,
          external_addr: SockAddr::from(match self.stream.local_addr() {
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
          tracing::debug!("-> player: id = {}, name = {}", info.id, info.name);

          if info.id != slot_info.slot_player_id {
            player_info_packets.push(Packet::simple(PlayerInfo::new(info.id, &info.name))?);

            player_skin_packets.push(Packet::simple(ProtoBufPayload::new(PlayerSkinsMessage {
              player_id: info.id as u32,
              ..Default::default()
            }))?);
          }

          player_profile_packets.push(Packet::simple(ProtoBufPayload::new(
            PlayerProfileMessage::new(info.id, &info.name),
          ))?);
        }

        replies.extend(player_info_packets);
        replies.extend(player_skin_packets);
        replies.extend(player_profile_packets);

        // map check
        replies.push(Packet::simple(MapCheck::new(
          map_checksum.file_size as u32,
          map_checksum.crc32,
          &game_settings,
        ))?);
        tracing::debug!(
          "-> map check: file_size = {}, crc32 = {}",
          map_checksum.file_size,
          map_checksum.crc32
        );

        self.stream.send_all(replies).await?;
      }
      MapSize::PACKET_TYPE_ID => {
        let payload: MapSize = pkt.decode_simple_payload()?;
        tracing::debug!("<- map size: {:?}", payload);
      }
      ChatToHost::PACKET_TYPE_ID => {
        self
          .stream
          .send(Packet::simple(ChatFromHost::chat(
            slot_info.slot_player_id,
            &[slot_info.slot_player_id],
            "This is a virtual LAN game, no one will hear you.",
          ))?)
          .await?;
      }
      ProtoBufPayload::PACKET_TYPE_ID => {
        let payload: ProtoBufPayload = pkt.decode_simple_payload()?;
        match payload.type_id {
          ProtoBufMessageTypeId::Unknown2 => {
            tracing::warn!("-> unexpected protobuf packet type: {:?}", payload.type_id)
          }
          ProtoBufMessageTypeId::PlayerProfile => {
            self.stream.send(pkt).await?;
            tracing::debug!("<-> self PlayerProfile ok");
          }
          ProtoBufMessageTypeId::PlayerSkins => {
            self.stream.send(pkt).await?;
            tracing::debug!("<-> self PlayerSkins ok");
          }
          ProtoBufMessageTypeId::PlayerUnknown5 => {
            self.stream.send(pkt).await?;
            tracing::debug!("<-> self PlayerUnknown5 ok");
          }
          ProtoBufMessageTypeId::UnknownValue(id) => {
            tracing::warn!("unexpected protobuf packet type id: {}", id)
          }
        }
      }
      _ => return Err(Error::UnexpectedW3GSPacket(pkt)),
    }
    Ok(())
  }
}
