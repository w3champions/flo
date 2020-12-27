use crate::controller::{ControllerClient, SendWs, UpdateMuteList};
use crate::error::*;
use crate::game::LocalGameInfo;
use crate::message::message;
use crate::message::message::OutgoingMessage;
use crate::node::{AddNode, GetNodePingMap, NodeRegistry, RemoveNode, UpdateNodes};
use crate::ping::PingUpdate;
use crate::platform::{CalcMapChecksum, GetClientPlatformInfo, Platform};
use crate::types::*;
use flo_net::packet::*;
use flo_net::proto::flo_connect as proto;
use flo_net::stream::FloStream;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};
use s2_grpc_utils::S2ProtoPack;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::delay_for;
use tracing_futures::Instrument;

pub struct ControllerStream {
  id: u64,
  domain: String,
  token: String,
  parent: Addr<ControllerClient>,
  frame_tx: Sender<Frame>,
  frame_rx: Option<Receiver<Frame>>,
  current_game_info: Option<Arc<LocalGameInfo>>,
  platform: Addr<Platform>,
  nodes: Addr<NodeRegistry>,
}

impl ControllerStream {
  pub fn new(
    parent: Addr<ControllerClient>,
    platform: Addr<Platform>,
    nodes: Addr<NodeRegistry>,
    id: u64,
    domain: &str,
    token: String,
  ) -> Self {
    let (frame_tx, frame_rx) = channel(5);
    Self {
      id,
      domain: domain.to_string(),
      token: token.to_string(),
      parent,
      frame_tx,
      frame_rx: Some(frame_rx),
      current_game_info: None,
      platform,
      nodes,
    }
  }

  async fn report_ping(
    id: u64,
    mut frame_tx: Sender<Frame>,
    parent: &Addr<ControllerClient>,
    nodes: &Addr<NodeRegistry>,
  ) -> Result<()> {
    let ping_map = nodes.send(GetNodePingMap).await??;
    parent
      .notify(SendWs::new(
        id,
        OutgoingMessage::PingUpdate(PingUpdate {
          ping_map: ping_map.clone(),
        }),
      ))
      .await?;
    frame_tx
      .send(
        proto::PacketPlayerPingMapUpdateRequest {
          ping_map: ping_map
            .into_iter()
            .map(|(k, v)| Ok((k, v.pack()?)))
            .collect::<Result<Vec<(i32, proto::PingStats)>>>()?
            .into_iter()
            .collect(),
        }
        .encode_as_frame()?,
      )
      .await
      .map_err(|_| Error::TaskCancelled(anyhow::format_err!("controller stream worker gone")))?;
    Ok(())
  }

  async fn connect_and_serve(
    id: u64,
    domain: &str,
    token: String,
    mut frame_receiver: Receiver<Frame>,
    owner: Addr<Self>,
    parent: Addr<ControllerClient>,
    nodes_reg: Addr<NodeRegistry>,
  ) -> Result<()> {
    let addr = format!("{}:{}", domain, flo_constants::CONTROLLER_SOCKET_PORT);
    tracing::debug!("connect addr: {}", addr);

    let mut stream = FloStream::connect_no_delay(addr).await?;

    tracing::debug!("connected");

    stream
      .send(proto::PacketClientConnect {
        connect_version: Some(crate::version::FLO_VERSION.into()),
        token,
      })
      .await?;

    let reply = stream.recv_frame().await?;

    let (session, nodes): (PlayerSession, _) = flo_net::try_flo_packet! {
      reply => {
        p: proto::PacketClientConnectAccept => {
          (
            PlayerSession::unpack(p.session)?,
            p.nodes
          )
        }
        p: proto::PacketClientConnectReject => {
          return Err(Error::ConnectionRequestRejected(S2ProtoEnum::unpack_enum(p.reason())))
        }
      }
    };

    let player_id = session.player.id;

    tracing::debug!(
      player_id,
      "player = {}, status = {:?}",
      session.player.id,
      session.status
    );

    parent
      .notify(ControllerEventData::Connected.wrap(id))
      .await?;
    parent
      .notify(
        ControllerEventData::PlayerSessionUpdate(PlayerSessionUpdateEvent::Full(session.clone()))
          .wrap(id),
      )
      .await?;
    parent.send(UpdateNodes { nodes }).await??;
    parent
      .notify(SendWs::new(
        id,
        message::OutgoingMessage::PlayerSession(session),
      ))
      .await?;

    loop {
      tokio::select! {
        next_send = frame_receiver.recv() => {
          if let Some(frame) = next_send {
            match stream.send_frame_timeout(frame).await {
              Ok(_) => {},
              Err(e) => {
                tracing::debug!("exiting: send error: {}", e);
                break;
              }
            }
          } else {
            tracing::debug!("exiting: sender dropped");
            break;
          }
        }
        recv = stream.recv_frame() => {
          match recv {
            Ok(mut frame) => {
              if frame.type_id == PacketTypeId::Ping {
                frame.type_id = PacketTypeId::Pong;
                match stream.send_frame_timeout(frame).await {
                  Ok(_) => {
                    continue;
                  },
                  Err(e) => {
                    tracing::debug!("exiting: send error: {}", e);
                    break;
                  }
                }
              }

              match Self::handle_frame(id, player_id, frame, &mut stream, &owner, &parent, &nodes_reg).await {
                Ok(_) => {},
                Err(e) => {
                  tracing::error!("handle frame: {}", e);
                }
              }
            },
            Err(e) => {
              tracing::debug!("exiting: recv: {}", e);
              break;
            }
          }
        }
      }
    }

    parent
      .notify(SendWs::new(
        id,
        OutgoingMessage::Disconnect(message::Disconnect {
          reason: message::DisconnectReason::Unknown,
          message: "Server connection closed".to_string(),
        }),
      ))
      .await?;

    parent
      .notify(ControllerEventData::Disconnected.wrap(id))
      .await?;

    tracing::debug!("exiting");

    Ok(())
  }

  // handle controller packets
  async fn handle_frame(
    id: u64,
    player_id: i32,
    frame: Frame,
    stream: &mut FloStream,
    owner: &Addr<Self>,
    parent: &Addr<ControllerClient>,
    nodes: &Addr<NodeRegistry>,
  ) -> Result<()> {
    flo_net::try_flo_packet! {
      frame => {
        p: proto::PacketClientDisconnect => {
          SendWs::new(id, OutgoingMessage::Disconnect(message::Disconnect {
              reason: S2ProtoEnum::unpack_i32(p.reason)?,
              message: format!("Server closed the connection: {:?}", p.reason)
            })).notify(parent).await?;
        }
        p: proto::PacketGameInfo => {
          parent.notify(ControllerEventData::SelectNode(p.game.as_ref().and_then(|g| {
            g.node.as_ref().map(|node| node.id)
          })).wrap(id)).await?;

          let game = GameInfo::unpack(p.game)?;

          let local_game_info = Arc::new(LocalGameInfo::from_game_info(player_id, &game)?);
          owner.send(SetLocalGameInfo(local_game_info.clone().into())).await??;

          SendWs::new(id, OutgoingMessage::CurrentGameInfo(game)).notify(parent).await?;
        }
        p: proto::PacketGamePlayerEnter => {
          let slot_index = p.slot_index;
          let slot = p.slot.clone();
          owner.send(UpdateLocalGameInfo::new(
            move |info| -> Result<_> {
              if let Some(r) = info.slots.get_mut(slot_index as usize) {
                *r = Slot::unpack(slot)?;
                Ok(())
              } else {
                tracing::error!("PacketGamePlayerEnter: invalid slot index: {}", slot_index);
                Err(Error::InvalidMapInfo)
              }
            }
          )).await??;
          SendWs::new(
            id,
            OutgoingMessage::GamePlayerEnter(S2ProtoUnpack::unpack(p)?)
          ).notify(parent).await?;
        }
        p: proto::PacketGamePlayerLeave => {
          owner.send(UpdateLocalGameInfo::new({
            let p = p.clone();
            move |info| -> Result<_> {
              if let Some(slot) = info.slots.iter_mut().find(|s| s.player.as_ref().map(|p| p.id) == Some(p.player_id)) {
                *slot = Slot::default();
                Ok(())
              } else {
                tracing::error!(game_id = p.game_id, player_id = p.player_id, "PacketGamePlayerLeave: player slot not found");
                Err(Error::InvalidMapInfo)
              }
            }
          })).await??;
          SendWs::new(
            id,
            OutgoingMessage::GamePlayerLeave(p)
          ).notify(parent).await?;
        }
        p: proto::PacketGameSlotUpdate => {
          owner.send(UpdateLocalGameInfo::new({
            let p = p.clone();
            move |info| -> Result<_> {
              if let Some(slot) = info.slots.get_mut(p.slot_index as usize) {
                slot.player = p.player.map(PlayerInfo::unpack).transpose()?;
                slot.settings = SlotSettings::unpack(p.slot_settings.clone())?;
                Ok(())
              } else {
                tracing::error!("PacketGamePlayerEnter: invalid slot index: {}", p.slot_index);
                Err(Error::InvalidMapInfo)
              }
            }
          })).await??;
          SendWs::new(
            id,
            OutgoingMessage::GameSlotUpdate(S2ProtoUnpack::unpack(p)?)
          ).notify(parent).await?;
        }
        p: proto::PacketPlayerSessionUpdate => {
          let session = PlayerSessionUpdate::unpack(p)?;
          parent.notify(ControllerEventData::PlayerSessionUpdate(PlayerSessionUpdateEvent::Partial(session.clone())).wrap(id)).await?;
          if session.game_id.is_none() {
            owner.send(SetLocalGameInfo(None)).await??;
          }
          SendWs::new(
            id,
            OutgoingMessage::PlayerSessionUpdate(session)
          ).notify(parent).await?;
        }
        p: proto::PacketListNodes => {
          parent
            .send(UpdateNodes{ nodes: p.nodes.clone() })
            .await??;
        }
        p: proto::PacketGameSelectNode => {
          parent.notify(ControllerEventData::SelectNode(p.node_id).wrap(id)).await?;
          owner.send(UpdateLocalGameInfo::new({
            let node_id = p.node_id;
            move |info| -> Result<_> {
              info.node_id = node_id;
              Ok(())
            }
          })).await??;
          SendWs::new(
            id,
            OutgoingMessage::GameSelectNode(p)
          ).notify(parent).await?;
        }
        p: proto::PacketPlayerPingMapUpdate => {
          SendWs::new(
            id,
            OutgoingMessage::PlayerPingMapUpdate(p)
          ).notify(parent).await?;
        }
        p: proto::PacketGamePlayerPingMapSnapshot => {
          SendWs::new(
            id,
            OutgoingMessage::GamePlayerPingMapSnapshot(p)
          ).notify(parent).await?;
        }
        p: proto::PacketGameStartReject => {
          SendWs::new(
            id,
            OutgoingMessage::GameStartReject(p)
          ).notify(parent).await?;
        }
        p: proto::PacketGameStarting => {
          let info = owner.send(GetGameStartClientInfo {
            game_id: p.game_id
          }).await??;
          if let Some(info) = info {
            stream.send(flo_net::proto::flo_connect::PacketGameStartPlayerClientInfoRequest {
              game_id: p.game_id,
              war3_version: info.war3_version,
              map_sha1: info.map_sha1,
            }).await?;
            SendWs::new(
              id,
              OutgoingMessage::GameStarting(p)
            ).notify(parent).await?;
          }
        }
        p: proto::PacketGamePlayerToken => {
          let info = owner.send(GetLocalGameInfo).await?;
          if let Some(info) = info {
            if info.game_id == p.game_id {
              parent.notify(ControllerEventData::GameReceived(GameReceivedEvent {
                node_id: p.node_id,
                game_info: info,
                player_token: p.player_token,
              }).wrap(id)).await?;
            } else {
              tracing::warn!("received player for game#{} but the active game id is {}", p.game_id, info.game_id);
            }
          } else {
            tracing::warn!("received player token but there is no active game");
          }
        }
        p: proto::PacketPlayerMuteListUpdate => {
          tracing::debug!("mute list update: {:?}", p.mute_list);
          parent.notify(UpdateMuteList {
            mute_list: p.mute_list
          }).await?;
        }
        // client status update from node
        p: proto::PacketGameSlotClientStatusUpdate => {
          SendWs::new(
            id,
            OutgoingMessage::GameSlotClientStatusUpdate(S2ProtoUnpack::unpack(p)?)
          ).notify(parent).await?;
        }
        p: flo_net::proto::flo_node::PacketNodeGameStatusUpdate => {
          SendWs::new(
            id,
            OutgoingMessage::GameStatusUpdate(p.into())
          ).notify(parent).await?;
        }
        p: proto::PacketAddNode => {
          nodes
            .notify(AddNode { node: p.node.extract()? })
            .await?;
        }
        p: proto::PacketRemoveNode => {
          nodes
            .notify(RemoveNode { node_id: p.node_id })
            .await?;
        }
      }
    };
    Ok(())
  }
}

#[async_trait]
impl Actor for ControllerStream {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let frame_rx = if let Some(rx) = self.frame_rx.take() {
      rx
    } else {
      let (frame_tx, frame_rx) = channel(5);
      self.frame_tx = frame_tx;
      frame_rx
    };

    ctx.spawn({
      let id = self.id;
      let frame_tx = self.frame_tx.clone();
      let parent = self.parent.clone();
      let nodes = self.nodes.clone();
      async move {
        delay_for(Duration::from_secs(2)).await;
        loop {
          if let Err(err) = Self::report_ping(id, frame_tx.clone(), &parent, &nodes).await {
            tracing::error!("report ping: {}", err)
          }
          delay_for(Duration::from_secs(5)).await;
        }
      }
    });

    ctx.spawn(
      {
        let id = self.id;
        let domain = self.domain.clone();
        let token = self.token.clone();
        let owner = ctx.addr();
        let parent = self.parent.clone();
        let nodes = self.nodes.clone();
        async move {
          if let Err(err) =
            Self::connect_and_serve(id, &domain, token, frame_rx, owner, parent.clone(), nodes)
              .await
          {
            tracing::error!("controller stream error: {}", err);

            SendWs::new(
              id,
              OutgoingMessage::ConnectRejected(message::ErrorMessage::new(match &err {
                Error::ConnectionRequestRejected(reason) => {
                  format!("server rejected: {:?}", reason)
                }
                other => other.to_string(),
              })),
            )
            .notify(&parent)
            .await
            .ok();

            parent
              .notify(ControllerEventData::ConnectionError(err).wrap(id))
              .await
              .ok();
          }

          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker", id = self.id)),
    );
  }
}

struct UpdateLocalGameInfo<F>
where
  F: FnOnce(&mut LocalGameInfo) -> Result<()>,
{
  f: F,
}

impl<F> UpdateLocalGameInfo<F>
where
  F: FnOnce(&mut LocalGameInfo) -> Result<()> + Send + 'static,
{
  fn new(f: F) -> Self {
    Self { f }
  }
}

impl<F> Message for UpdateLocalGameInfo<F>
where
  F: FnOnce(&mut LocalGameInfo) -> Result<()> + Send + 'static,
{
  type Result = Result<Arc<LocalGameInfo>>;
}

#[async_trait]
impl<F> Handler<UpdateLocalGameInfo<F>> for ControllerStream
where
  F: FnOnce(&mut LocalGameInfo) -> Result<()> + Send + 'static,
{
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateLocalGameInfo { f }: UpdateLocalGameInfo<F>,
  ) -> <UpdateLocalGameInfo<F> as Message>::Result {
    let r = self
      .current_game_info
      .clone()
      .ok_or_else(|| Error::LocalGameInfoNotFound)?;
    let mut mutated = r.clone();

    f(Arc::make_mut(&mut mutated))?;
    if !Arc::ptr_eq(&r, &mutated) {
      self.current_game_info.replace(mutated.clone());
    }

    self
      .parent
      .notify(
        ControllerEventData::GameInfoUpdate(GameInfoUpdateEvent {
          game_info: Some(mutated.clone()),
        })
        .wrap(self.id),
      )
      .await?;

    Ok(mutated)
  }
}

struct SetLocalGameInfo(Option<Arc<LocalGameInfo>>);

impl Message for SetLocalGameInfo {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SetLocalGameInfo> for ControllerStream {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SetLocalGameInfo(info): SetLocalGameInfo,
  ) -> <SetLocalGameInfo as Message>::Result {
    if let Some(info) = info {
      self
        .parent
        .notify(ControllerEventData::SelectNode(info.node_id.clone()).wrap(self.id))
        .await?;
      self
        .parent
        .notify(
          ControllerEventData::GameInfoUpdate(GameInfoUpdateEvent {
            game_info: Some(info.clone()),
          })
          .wrap(self.id),
        )
        .await?;
      self.current_game_info.replace(info);
    } else {
      self.current_game_info.take();
      self
        .parent
        .notify(ControllerEventData::SelectNode(None).wrap(self.id))
        .await?;
      self
        .parent
        .notify(
          ControllerEventData::GameInfoUpdate(GameInfoUpdateEvent { game_info: None })
            .wrap(self.id),
        )
        .await?;
    }
    Ok(())
  }
}

struct GetLocalGameInfo;

impl Message for GetLocalGameInfo {
  type Result = Option<Arc<LocalGameInfo>>;
}

#[async_trait]
impl Handler<GetLocalGameInfo> for ControllerStream {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetLocalGameInfo,
  ) -> <GetLocalGameInfo as Message>::Result {
    self.current_game_info.clone()
  }
}

struct GetGameStartClientInfo {
  game_id: i32,
}

struct GameStartClientInfo {
  war3_version: String,
  map_sha1: Vec<u8>,
}

impl Message for GetGameStartClientInfo {
  type Result = Result<Option<GameStartClientInfo>>;
}

#[async_trait]
impl Handler<GetGameStartClientInfo> for ControllerStream {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetGameStartClientInfo { game_id }: GetGameStartClientInfo,
  ) -> <GetGameStartClientInfo as Message>::Result {
    if let Some(info) = self.current_game_info.as_ref() {
      if info.game_id == game_id {
        let client_info = self
          .platform
          .send(GetClientPlatformInfo { force_reload: true })
          .await?
          .map_err(|_| Error::War3NotLocated)?;
        let war3_version = client_info.version;
        let map_sha1 = self
          .platform
          .send(CalcMapChecksum {
            path: info.map_path.clone(),
          })
          .await??
          .sha1
          .to_vec();
        return Ok(Some(GameStartClientInfo {
          war3_version,
          map_sha1,
        }));
      }
    }
    Ok(None)
  }
}

pub struct SendFrame(pub Frame);

impl Message for SendFrame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SendFrame> for ControllerStream {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SendFrame(frame): SendFrame,
  ) -> <SendFrame as Message>::Result {
    match self.frame_tx.send(frame).await {
      Ok(_) => Ok(()),
      Err(_) => Err(Error::TaskCancelled(anyhow::format_err!(
        "controller stream worker gone"
      ))),
    }
  }
}

#[derive(Debug)]
pub struct ControllerEvent {
  pub id: u64,
  pub data: ControllerEventData,
}

impl Message for ControllerEvent {
  type Result = ();
}

#[derive(Debug)]
pub enum ControllerEventData {
  Connected,
  ConnectionError(Error),
  PlayerSessionUpdate(PlayerSessionUpdateEvent),
  GameInfoUpdate(GameInfoUpdateEvent),
  GameReceived(GameReceivedEvent),
  SelectNode(Option<i32>),
  Disconnected,
}

impl ControllerEventData {
  fn wrap(self, id: u64) -> ControllerEvent {
    ControllerEvent { id, data: self }
  }
}

#[derive(Debug)]
pub enum PlayerSessionUpdateEvent {
  Full(PlayerSession),
  Partial(PlayerSessionUpdate),
}

#[derive(Debug)]
pub struct GameInfoUpdateEvent {
  pub game_info: Option<Arc<LocalGameInfo>>,
}

#[derive(Debug)]
pub struct GameReceivedEvent {
  pub node_id: i32,
  pub game_info: Arc<LocalGameInfo>,
  pub player_token: Vec<u8>,
}
