use crate::error::*;
use crate::ping::{GetPingMap, PingActor, SetActiveAddress, UpdateAddresses};
use flo_net::proto::flo_connect::Node;
use flo_state::{async_trait, Actor, Container, Context, Handler, Message, RegistryRef, Service};
use flo_types::ping::PingStats;
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

pub struct NodeRegistry {
  map: BTreeMap<i32, NodeInfo>,
  ping: Container<PingActor>,
}

impl NodeRegistry {
  pub fn new() -> Self {
    Self {
      map: Default::default(),
      ping: PingActor::new().start(),
    }
  }
}

impl Actor for NodeRegistry {}

#[async_trait]
impl Service for NodeRegistry {
  type Error = Error;

  async fn create(_: &mut RegistryRef<()>) -> Result<Self, Self::Error> {
    Ok(NodeRegistry::new())
  }
}

pub struct GetNode {
  pub node_id: i32,
}

impl Message for GetNode {
  type Result = Option<NodeInfo>;
}

#[async_trait]
impl Handler<GetNode> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetNode { node_id }: GetNode,
  ) -> <GetNode as Message>::Result {
    self.map.get(&node_id).cloned()
  }
}

#[derive(Debug)]
pub struct UpdateNodes {
  pub nodes: Vec<Node>,
}

impl Message for UpdateNodes {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<UpdateNodes> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateNodes { nodes }: UpdateNodes,
  ) -> <UpdateNodes as Message>::Result {
    let remove_ids: Vec<i32> = nodes
      .iter()
      .filter_map(|n| {
        if !self.map.contains_key(&n.id) {
          Some(n.id)
        } else {
          None
        }
      })
      .collect();

    for id in remove_ids {
      self.map.remove(&id);
    }

    for node in nodes {
      let name = node.name;
      let ip_str: String = node.ip_addr;
      let id = node.id;
      let (ip, port) = if ip_str.contains(":") {
        let addr = if let Some(addr) = ip_str.parse::<SocketAddrV4>().ok() {
          addr
        } else {
          tracing::warn!("skipped node {}, parse addr with port failed.", node.id);
          continue;
        };

        (
          addr.ip().clone(),
          addr.port() + flo_constants::NODE_ECHO_PORT_OFFSET,
        )
      } else {
        let addr: Ipv4Addr = if let Some(addr) = ip_str.parse::<Ipv4Addr>().ok() {
          addr
        } else {
          tracing::warn!("skipped node {}, parse ip addr failed.", node.id);
          continue;
        };
        let port = flo_constants::NODE_ECHO_PORT;
        (addr, port)
      };

      self.map.insert(
        id,
        NodeInfo {
          id,
          name: name.to_string(),
          socket_addr: SocketAddr::from((ip, port)),
        },
      );
    }

    let addresses: Vec<_> = self.map.values().map(|v| v.socket_addr).collect();
    self.ping.send(UpdateAddresses { addresses }).await?;

    Ok(())
  }
}

pub struct SetActiveNode {
  pub node_id: Option<i32>,
}

impl Message for SetActiveNode {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SetActiveNode> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SetActiveNode { node_id }: SetActiveNode,
  ) -> <SetActiveNode as Message>::Result {
    if let Some(node_id) = node_id {
      if let Some(node) = self.map.get(&node_id) {
        self
          .ping
          .send(SetActiveAddress {
            address: Some(node.socket_addr),
          })
          .await??;
      } else {
        self.ping.send(SetActiveAddress { address: None }).await??;
      }
    } else {
      self.ping.send(SetActiveAddress { address: None }).await??;
    }
    Ok(())
  }
}

pub struct GetNodePingMap;
impl Message for GetNodePingMap {
  type Result = Result<BTreeMap<i32, PingStats>>;
}

#[async_trait]
impl Handler<GetNodePingMap> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetNodePingMap,
  ) -> <GetNodePingMap as Message>::Result {
    let ping_map = self.ping.send(GetPingMap).await?;
    Ok(
      self
        .map
        .iter()
        .filter_map(|(id, node)| {
          ping_map
            .get(&node.socket_addr)
            .cloned()
            .map(|stats| (*id, stats))
        })
        .collect(),
    )
  }
}

pub struct UpdateAddressesAndGetNodePingMap(pub UpdateNodes);
impl Message for UpdateAddressesAndGetNodePingMap {
  type Result = Result<BTreeMap<i32, PingStats>>;
}

#[async_trait]
impl Handler<UpdateAddressesAndGetNodePingMap> for NodeRegistry {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    UpdateAddressesAndGetNodePingMap(update): UpdateAddressesAndGetNodePingMap,
  ) -> <UpdateAddressesAndGetNodePingMap as Message>::Result {
    self.handle(ctx, update).await?;

    let ping_map = self.ping.send(GetPingMap).await?;
    Ok(
      self
        .map
        .iter()
        .filter_map(|(id, node)| {
          ping_map
            .get(&node.socket_addr)
            .cloned()
            .map(|stats| (*id, stats))
        })
        .collect(),
    )
  }
}

#[derive(Debug, Serialize)]
pub struct PingUpdate {
  pub node_id: i32,
  pub ping: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
  pub id: i32,
  pub name: String,
  socket_addr: SocketAddr,
}

impl NodeInfo {
  pub fn client_socket_addr(&self) -> SocketAddr {
    self.socket_addr_offset(flo_constants::NODE_CLIENT_PORT_OFFSET)
  }

  fn socket_addr_offset(&self, offset: u16) -> SocketAddr {
    let mut addr = self.socket_addr;
    addr.set_port(addr.port() + offset);
    addr
  }
}
