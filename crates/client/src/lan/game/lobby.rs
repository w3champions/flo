use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::sync::watch::Receiver;
use tokio::time::{interval_at, sleep};

use flo_util::binary::SockAddr;
use flo_w3gs::net::W3GSStream;
use flo_w3gs::protocol::chat::{ChatFromHost, ChatToHost};
use flo_w3gs::protocol::game::{CountDownEnd, CountDownStart};
use flo_w3gs::protocol::join::{ReqJoin, SlotInfoJoin};
use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};
use flo_w3gs::protocol::map::{MapCheck, MapSize};
use flo_w3gs::protocol::packet::*;
use flo_w3gs::protocol::ping::{PingFromHost, PongToHost};
use flo_w3gs::protocol::player::{PlayerInfo, PlayerProfileMessage, PlayerSkinsMessage};

use crate::error::*;
use crate::lan::game::LanGameInfo;
use crate::node::stream::NodeStreamSender;
use flo_types::node::{NodeGameStatus, SlotClientStatus};
use flo_w3gs::protocol::constants::ProtoBufMessageTypeId;

const LOBBY_PING_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub enum LobbyAction {
  Start,
  Leave,
}

#[derive(Debug)]
pub struct LobbyHandler<'a> {
  info: &'a LanGameInfo,
  stream: &'a mut W3GSStream,
  node_stream: Option<&'a mut NodeStreamSender>,
  status_rx: &'a mut Receiver<Option<NodeGameStatus>>,
  starting: bool,
}

impl<'a> LobbyHandler<'a> {
  pub fn new(
    info: &'a LanGameInfo,
    stream: &'a mut W3GSStream,
    node_stream: Option<&'a mut NodeStreamSender>,
    status_rx: &'a mut Receiver<Option<NodeGameStatus>>,
  ) -> Self {
    LobbyHandler {
      info,
      stream,
      node_stream,
      status_rx,
      starting: false,
    }
  }

  pub async fn run(&mut self) -> Result<LobbyAction> {
    let initial_game_state = { self.status_rx.borrow().clone() };
    let mut join_state =
      JoinPacketRecvState::new(initial_game_state, self.info.slot_info.player_infos.len());
    let mut ping_interval = interval_at(
      (Instant::now() + LOBBY_PING_INTERVAL).into(),
      LOBBY_PING_INTERVAL,
    );
    let base_t = Instant::now();
    let mut reported = false;

    loop {
      tokio::select! {
        next = self.stream.recv() => {
          let pkt = next?;
          if let Some(pkt) = pkt {
            if pkt.type_id() == LeaveReq::PACKET_TYPE_ID {
              if let Some(node_stream) = self.node_stream.as_mut() {
                node_stream.report_slot_status(SlotClientStatus::Connected).await.ok();
              }
              self.stream.send(Packet::simple(LeaveAck)?).await?;
              self.stream.flush().await?;
              return Ok(LobbyAction::Leave)
            }

            self.handle_packet(&mut join_state, base_t, pkt).await?;
            if join_state.is_ready() {
              // report to node that all players have joined
              if !reported {
                tracing::debug!("all join packets received");
                if let Some(node_stream) = self.node_stream.as_mut() {
                  node_stream.report_slot_status(SlotClientStatus::Joined).await.ok();
                }
                reported = true;
              }
              if join_state.should_start() {
                self.send_start().await?;
                return Ok(LobbyAction::Start)
              }
            }
          } else {
            return Err(Error::StreamClosed)
          }
        }
        _ = ping_interval.tick() => {
          self.stream.send(Packet::simple(PingFromHost::with_payload_since(base_t))?).await?;
        }
        ch = self.status_rx.changed() => {
          match ch {
            Ok(_) => {
              let next = self.status_rx.borrow().clone();
              match next {
                Some(status) => {
                  join_state.status = Some(status);
                  if join_state.should_start() {
                    self.send_start().await?;
                    return Ok(LobbyAction::Start)
                  }
                },
                None => {},
              }
            },
            Err(_why) => {
              return Err(Error::TaskCancelled(anyhow::format_err!("game status tx dropped")))
            }
          }
        }
      }
    }
  }

  async fn send_start(&mut self) -> Result<()> {
    if self.starting {
      return Ok(());
    }
    self.starting = true;
    self.stream.send(Packet::simple(CountDownStart)?).await?;
    sleep(Duration::from_secs(6)).await;
    self.stream.send(Packet::simple(CountDownEnd)?).await?;
    Ok(())
  }

  async fn handle_packet(
    &mut self,
    state: &mut JoinPacketRecvState,
    base_t: Instant,
    pkt: Packet,
  ) -> Result<()> {
    let &LanGameInfo {
      ref slot_info,
      ref map_checksum,
      ref game_settings,
      ..
    } = self.info;

    match pkt.type_id() {
      ReqJoin::PACKET_TYPE_ID => {
        let num_players = slot_info.player_infos.len();
        let mut replies = Vec::with_capacity(num_players * 3);

        // slot info
        replies.push(Packet::simple(SlotInfoJoin {
          slot_info: slot_info.slot_info.clone(),
          player_id: slot_info.my_slot_player_id,
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
          if info.slot_player_id != slot_info.my_slot_player_id {
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
          }

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
        let payload: MapSize = pkt.decode_simple()?;
        tracing::debug!("<- map size: {:?}", payload);
      }
      ChatToHost::PACKET_TYPE_ID => {
        self
          .stream
          .send(Packet::simple(ChatFromHost::lobby(
            slot_info.my_slot_player_id,
            &[slot_info.my_slot_player_id],
            "Setting changes and chat are disabled.",
          ))?)
          .await?;
      }
      PongToHost::PACKET_TYPE_ID => {
        let payload: PongToHost = pkt.decode_simple()?;
        let _ping = payload.elapsed_millis(base_t);
      }
      ProtoBufPayload::PACKET_TYPE_ID => {
        let payload: ProtoBufPayload = pkt.decode_simple()?;
        match payload.type_id {
          ProtoBufMessageTypeId::Unknown2 => {
            tracing::warn!("-> unexpected protobuf packet type: {:?}", payload.type_id)
          }
          ProtoBufMessageTypeId::PlayerProfile => {
            state.num_profile = state.num_profile + 1;
            #[cfg(debug_assertions)]
            {
              tracing::debug!(
                "<-> PlayerProfile: {:?}",
                payload.decode_message::<PlayerProfileMessage>()?
              );
            }
            self.stream.send(pkt).await?;
          }
          ProtoBufMessageTypeId::PlayerSkins => {
            state.num_skins = state.num_skins + 1;
            self.stream.send(pkt).await?;
            #[cfg(debug_assertions)]
            {
              tracing::debug!(
                "<-> PlayerSkins: {:?}",
                payload.decode_message::<PlayerSkinsMessage>()?
              );
            }
          }
          ProtoBufMessageTypeId::PlayerUnknown5 => {
            state.num_unk5 = state.num_unk5 + 1;
            self.stream.send(pkt).await?;
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
      _ => return Err(Error::UnexpectedW3GSPacket(pkt)),
    }
    Ok(())
  }
}

#[derive(Debug)]
struct JoinPacketRecvState {
  total_players: usize,
  num_profile: usize,
  num_skins: usize,
  num_unk5: usize,
  status: Option<NodeGameStatus>,
}

impl JoinPacketRecvState {
  fn new(initial_game_state: Option<NodeGameStatus>, total_players: usize) -> Self {
    JoinPacketRecvState {
      total_players,
      num_profile: 0,
      num_skins: 0,
      num_unk5: 0,
      status: initial_game_state,
    }
  }

  fn is_ready(&self) -> bool {
    self.num_profile == self.total_players && self.num_skins == 1 && self.num_unk5 == 1
  }

  fn should_start(&self) -> bool {
    self.is_ready()
      && match self.status {
        Some(NodeGameStatus::Loading) => true,
        Some(NodeGameStatus::Running) => true,
        _ => false,
      }
  }
}
