use crate::error::*;
use crate::ping::{
  AddAddress, GetPingMap, PingActor, RemoveAddress, SetActiveAddress, UpdateAddresses,
};
use crate::StartConfig;
use flo_net::proto::flo_connect::Node;
use flo_state::{async_trait, Actor, Context, Handler, Message, Owner, RegistryRef, Service};
use flo_types::ping::PingStats;
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

pub struct NodeRegistry {
  map: BTreeMap<i32, NodeInfo>,
  addr_overrides: BTreeMap<i32, SocketAddr>,
  ping: Owner<PingActor>,
}

impl NodeRegistry {
  pub fn new() -> Self {
    Self {
      map: Default::default(),
      addr_overrides: Default::default(),
      ping: PingActor::new().start(),
    }
  }
}

impl Actor for NodeRegistry {}

#[async_trait]
impl Service<StartConfig> for NodeRegistry {
  type Error = Error;

  async fn create(_: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
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
    self.map.get(&node_id).cloned().map(|mut info| {
      if let Some(addr) = self.addr_overrides.get(&info.id) {
        info.socket_addr = *addr;
        tracing::debug!(node_id, "using override address: {:?}", addr);
      }
      info
    })
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
      let socket_addr = match parse_node_addr(&node) {
        Ok(v) => v,
        Err(err) => {
          tracing::error!(node_id = node.id, "skip node: {}", err);
          continue;
        }
      };
      let name = node.name;

      self.map.insert(
        node.id,
        NodeInfo {
          id: node.id,
          name: name.to_string(),
          location: node.location.to_string(),
          country_id: node.country_id.to_string(),
          socket_addr,
        },
      );
    }

    let addresses: Vec<_> = self
      .map
      .values()
      .map(|v| {
        self
          .addr_overrides
          .get(&v.id)
          .cloned()
          .unwrap_or_else(|| v.socket_addr)
      })
      .collect();
    self.ping.send(UpdateAddresses { addresses }).await?;

    Ok(())
  }
}

fn parse_node_addr(node: &Node) -> Result<SocketAddr> {
  let ip_str: &str = &node.ip_addr;
  let (ip, port) = if ip_str.contains(":") {
    let addr = if let Some(addr) = ip_str.parse::<SocketAddrV4>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeConfig);
    };

    (
      addr.ip().clone(),
      addr.port() + flo_constants::NODE_ECHO_PORT_OFFSET,
    )
  } else {
    let addr: Ipv4Addr = if let Some(addr) = ip_str.parse::<Ipv4Addr>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeConfig);
    };
    let port = flo_constants::NODE_ECHO_PORT;
    (addr, port)
  };
  Ok(SocketAddr::from((ip, port)))
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
          let addr = self
            .addr_overrides
            .get(id)
            .cloned()
            .unwrap_or_else(|| node.socket_addr.clone());

          ping_map.get(&addr).cloned().map(|stats| (*id, stats))
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
          let addr = self
            .addr_overrides
            .get(id)
            .cloned()
            .unwrap_or_else(|| node.socket_addr.clone());
          ping_map.get(&addr).cloned().map(|stats| (*id, stats))
        })
        .collect(),
    )
  }
}

pub struct AddNode {
  pub node: Node,
}

impl Message for AddNode {
  type Result = ();
}

#[async_trait]
impl Handler<AddNode> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    AddNode { node }: AddNode,
  ) -> <AddNode as Message>::Result {
    let socket_addr = match parse_node_addr(&node) {
      Ok(v) => v,
      Err(err) => {
        tracing::error!(node_id = node.id, "skip node: {}", err);
        return;
      }
    };
    let name = node.name;

    self.map.insert(
      node.id,
      NodeInfo {
        id: node.id,
        name: name.to_string(),
        location: node.location.to_string(),
        country_id: node.country_id.to_string(),
        socket_addr,
      },
    );

    self
      .ping
      .notify(AddAddress {
        address: socket_addr,
      })
      .await
      .ok();
    tracing::debug!(node_id = node.id, "add node: {}", socket_addr);
  }
}

pub struct RemoveNode {
  pub node_id: i32,
}

impl Message for RemoveNode {
  type Result = ();
}

#[async_trait]
impl Handler<RemoveNode> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    RemoveNode { node_id }: RemoveNode,
  ) -> <RemoveNode as Message>::Result {
    if let Some(node) = self.map.remove(&node_id) {
      self
        .ping
        .notify(RemoveAddress {
          address: node.socket_addr,
        })
        .await
        .ok();
      tracing::debug!(node_id, "remove node: {}", node.socket_addr);
    } else {
      tracing::warn!(node_id, "removed node was not found");
    }
  }
}

pub struct SetNodeAddrOverrides {
  pub overrides: BTreeMap<i32, SocketAddr>,
}

impl Message for SetNodeAddrOverrides {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<SetNodeAddrOverrides> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SetNodeAddrOverrides { overrides }: SetNodeAddrOverrides,
  ) -> <SetNodeAddrOverrides as Message>::Result {
    let mut addresses: Vec<_> = self.map.values().map(|v| v.socket_addr).collect();
    for (id, addr) in overrides.iter() {
      if !addresses.contains(addr) {
        tracing::debug!(node_id = *id, "addr override: {}", addr);
        addresses.push(*addr);
      }
    }
    self.addr_overrides = overrides;
    self.ping.notify(UpdateAddresses { addresses }).await?;
    Ok(())
  }
}

pub struct ClearNodeAddrOverrides;

impl Message for ClearNodeAddrOverrides {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<ClearNodeAddrOverrides> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: ClearNodeAddrOverrides,
  ) -> <SetNodeAddrOverrides as Message>::Result {
    self.addr_overrides.clear();

    let addresses: Vec<_> = self.map.values().map(|v| v.socket_addr).collect();
    self.ping.notify(UpdateAddresses { addresses }).await?;

    Ok(())
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
  pub location: String,
  pub country_id: String,
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
