use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Semaphore};
use tracing_futures::Instrument;

use flo_net::proto::flo_connect::Node;

use crate::error::*;
use crate::node::ping::Pinger;

pub type NodesConfigSenderRef = Arc<watch::Sender<Vec<Node>>>;
pub type NodeRegistryRef = Arc<NodeRegistry>;
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(30);
const ACTIVE_PING_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct NodeRegistry {
  ping_interval: Duration,
  ping_limiter: Arc<Semaphore>,
  state: Arc<RwLock<HashMap<i32, LoadedNode>>>,
  nodes_sender_ref: NodesConfigSenderRef,
  selected_node_id: RwLock<Option<i32>>,
}

impl NodeRegistry {
  pub fn new(ping_update_sender: mpsc::Sender<PingUpdate>) -> Self {
    let ping_limiter = Arc::new(Semaphore::new(3));
    let (nodes_sender, mut nodes_receiver) = watch::channel::<Vec<Node>>(vec![]);
    let nodes_sender_ref = Arc::new(nodes_sender);
    let state = Arc::new(RwLock::new(HashMap::<i32, LoadedNode>::new()));

    // worker to update nodes_storage after receiving `PacketListNodes`
    tokio::spawn({
      let ping_limiter = ping_limiter.clone();
      let state = state.clone();
      async move {
        loop {
          let nodes = nodes_receiver.recv().await;
          match nodes {
            Some(nodes) => {
              let mut guard = state.write();

              let remove_ids: Vec<i32> = nodes
                .iter()
                .filter_map(|n| {
                  if !guard.contains_key(&n.id) {
                    Some(n.id)
                  } else {
                    None
                  }
                })
                .collect();

              for id in remove_ids {
                guard.remove(&id);
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

                  (addr.ip().clone(), addr.port())
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

                guard
                  .entry(id)
                  .and_modify(|loaded| {
                    if loaded.info.name != name || loaded.info.ip != ip || loaded.info.port != port
                    {
                      let interval = loaded.pinger.current_interval();
                      let info = Arc::make_mut(&mut loaded.info);
                      info.ip = ip;
                      info.port = port;
                      if info.name != name {
                        info.name = name.clone();
                      }
                      loaded.pinger = Pinger::new(
                        ping_limiter.clone(),
                        interval,
                        ping_update_sender.clone(),
                        id,
                        ip,
                        port,
                      );
                    }
                  })
                  .or_insert_with(|| LoadedNode {
                    info: Arc::new(NodeInfo {
                      id,
                      name: name.to_string(),
                      ip,
                      port,
                    }),
                    pinger: Pinger::new(
                      ping_limiter.clone(),
                      DEFAULT_PING_INTERVAL,
                      ping_update_sender.clone(),
                      id,
                      ip,
                      port,
                    ),
                  });
              }
            }
            None => {
              tracing::debug!("exiting: sender dropped");
              break;
            }
          }
        }
      }
      .instrument(tracing::debug_span!("nodes_update_worker"))
    });

    Self {
      ping_interval: DEFAULT_PING_INTERVAL,
      ping_limiter,
      state,
      nodes_sender_ref,
      selected_node_id: RwLock::new(None),
    }
  }

  pub fn into_ref(self) -> NodeRegistryRef {
    Arc::new(self)
  }

  pub fn update_nodes(&self, nodes: Vec<Node>) -> Result<()> {
    self
      .nodes_sender_ref
      .broadcast(nodes)
      .map_err(|_| Error::BroadcastNodesConfigFailed)
  }

  pub fn get_current_ping(&self, node_id: i32) -> Option<u32> {
    self
      .state
      .read()
      .get(&node_id)
      .and_then(|node| node.pinger.current_ping())
  }

  pub fn get_node(&self, node_id: i32) -> Option<Arc<NodeInfo>> {
    self
      .state
      .read()
      .get(&node_id)
      .map(|loaded| loaded.info.clone())
  }

  pub fn set_selected_node(&self, node: Option<i32>) -> Result<()> {
    match node {
      Some(id) => {
        let mut guard = self.selected_node_id.write();
        let storage_guard = self.state.read();

        if let Some(prev_id) = guard.take() {
          if prev_id == id {
            return Ok(());
          }

          if let Some(loaded) = storage_guard.get(&prev_id) {
            loaded.pinger.set_interval(DEFAULT_PING_INTERVAL, false)?;
          }
        }

        if let Some(loaded) = storage_guard.get(&id) {
          loaded.pinger.set_interval(ACTIVE_PING_INTERVAL, true)?;
          *guard = Some(id);
        } else {
          return Err(Error::InvalidSelectedNodeId(id));
        }
      }
      None => {
        if let Some(id) = self.selected_node_id.write().take() {
          let storage_guard = self.state.read();
          if let Some(loaded) = storage_guard.get(&id) {
            loaded.pinger.set_interval(DEFAULT_PING_INTERVAL, false)?;
          }
        }
      }
    }
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
  pub ip: Ipv4Addr,
  pub port: u16,
}

#[derive(Debug)]
struct LoadedNode {
  info: Arc<NodeInfo>,
  pinger: Pinger,
}
