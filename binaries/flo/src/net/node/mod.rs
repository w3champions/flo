mod ping;

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Semaphore};
use tracing_futures::Instrument;

use flo_net::proto::flo_connect::{Node, SelectedNode};

use crate::error::*;
use ping::Pinger;

pub type NodesConfigSenderRef = Arc<watch::Sender<Vec<Node>>>;
pub type NodeRegistryRef = Arc<NodeRegistry>;
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(60);
const ACTIVE_PING_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct NodeRegistry {
  ping_interval: Duration,
  ping_limiter: Arc<Semaphore>,
  storage: Arc<RwLock<HashMap<i32, LoadedNode>>>,
  nodes_sender_ref: NodesConfigSenderRef,
  select_node_id: RwLock<Option<i32>>,
}

impl NodeRegistry {
  pub fn new(ping_update_sender: mpsc::Sender<PingUpdate>) -> Self {
    let ping_limiter = Arc::new(Semaphore::new(3));
    let (nodes_sender, mut nodes_receiver) = watch::channel::<Vec<Node>>(vec![]);
    let nodes_sender_ref = Arc::new(nodes_sender);
    let storage = Arc::new(RwLock::new(HashMap::<i32, LoadedNode>::new()));

    // worker to update nodes_storage after receiving `PacketListNodes`
    tokio::spawn({
      let ping_limiter = ping_limiter.clone();
      let storage = storage.clone();
      async move {
        loop {
          let nodes = nodes_receiver.recv().await;
          match nodes {
            Some(nodes) => {
              let mut guard = storage.write();

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
                    if loaded.ip != ip || loaded.port != port {
                      let interval = loaded.pinger.current_interval();
                      loaded.ip = ip;
                      loaded.port = port;
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
                    id,
                    ip,
                    port,
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
      storage,
      nodes_sender_ref,
      select_node_id: RwLock::new(None),
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

  pub fn set_selected_node(&self, node: Option<SelectedNode>) -> Result<()> {
    match node {
      Some(SelectedNode { id: Some(id), .. }) => {
        let mut guard = self.select_node_id.write();
        let storage_guard = self.storage.read();

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
      Some(_) => return Err(Error::CustomNodeUnimplemented),
      None => {
        if let Some(id) = self.select_node_id.write().take() {
          let storage_guard = self.storage.read();
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

#[derive(Debug)]
struct LoadedNode {
  id: i32,
  ip: Ipv4Addr,
  port: u16,
  pinger: Pinger,
}
