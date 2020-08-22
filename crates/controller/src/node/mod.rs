pub mod conn;
pub mod db;
mod types;
pub use types::*;

use backoff::ExponentialBackoff;
use bs_diesel_utils::ExecutorRef;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::Notify;

use crate::error::*;
use crate::node::NodeConnConfig;

use backoff::backoff::Backoff;
use conn::{NodeConn, NodeConnEvent, NodeConnEventData};
use tracing_futures::Instrument;

#[derive(Debug)]
pub struct NodeRegistry {
  state: Arc<State>,
}

pub type NodeRegistryRef = Arc<NodeRegistry>;

impl NodeRegistry {
  pub async fn init(db: ExecutorRef) -> Result<Self> {
    let (event_sender, mut event_receiver) = channel(2);

    let configs = db
      .exec(|conn| crate::node::db::get_node_conn_configs(conn))
      .await?;

    let state = Arc::new(State::new(event_sender, configs)?);

    tokio::spawn(
      {
        let state = state.clone();
        async move {
          while let Some(event) = event_receiver.recv().await {
            let node_id = event.node_id;
            match event.data {
              NodeConnEventData::WorkerErrorEvent(err) => {
                tracing::error!(node_id, "worker: {}", err);
                if let Err(err) = state.reconnect(node_id) {
                  tracing::error!(node_id, "reconnect: {}", err);
                }
              }
              NodeConnEventData::DisconnectedEvent => {
                tracing::error!(node_id, "disconnected");
                if let Err(err) = state.reconnect(node_id) {
                  tracing::error!(node_id, "reconnect: {}", err);
                }
              }
              NodeConnEventData::ConnectedEvent => {
                if let Err(err) = state.reset_backoff(node_id) {
                  tracing::error!(node_id, "reset backoff: {}", err);
                }
              }
            }
          }
          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("event_handler_worker")),
    );

    Ok(NodeRegistry { state })
  }

  pub fn get_conn(&self, node_id: i32) -> Result<Arc<NodeConn>> {
    self.state.get_conn(node_id)
  }

  pub fn into_ref(self) -> NodeRegistryRef {
    Arc::new(self)
  }
}

impl Drop for NodeRegistry {
  fn drop(&mut self) {
    self.state.dropper.notify();
  }
}

#[derive(Debug)]
struct State {
  dropper: Notify,
  root_event_sender: Sender<NodeConnEvent>,
  map: HashMap<i32, RwLock<NodeSlot>>,
}

impl State {
  fn new(root_event_sender: Sender<NodeConnEvent>, configs: Vec<NodeConnConfig>) -> Result<Self> {
    let mut map = HashMap::new();
    for config in configs {
      let conn = Arc::new(NodeConn::new(
        config.id,
        &config.addr,
        &config.secret,
        root_event_sender.clone().into(),
        None,
      )?);
      map.insert(
        config.id,
        RwLock::new(NodeSlot {
          config,
          conn,
          reconnect_backoff: ExponentialBackoff {
            initial_interval: Duration::from_secs(5),
            current_interval: Duration::from_secs(5),
            multiplier: 2.0,
            ..Default::default()
          },
        }),
      );
    }

    Ok(State {
      dropper: Notify::new(),
      map,
      root_event_sender,
    })
  }

  fn get_conn(&self, node_id: i32) -> Result<Arc<NodeConn>> {
    self
      .map
      .get(&node_id)
      .map(|slot| slot.read().conn.clone())
      .ok_or_else(|| Error::NodeNotFound)
  }

  fn reconnect(&self, node_id: i32) -> Result<()> {
    if let Some(slot) = self.map.get(&node_id) {
      let mut guard = slot.write();
      let delay = guard.reconnect_backoff.next_backoff();
      tracing::debug!(node_id, "reconnect backoff: {:?}", delay);
      guard.conn = Arc::new(NodeConn::new(
        node_id,
        &guard.config.addr,
        &guard.config.secret,
        self.root_event_sender.clone().into(),
        delay,
      )?);
      Ok(())
    } else {
      Err(Error::NodeNotFound)
    }
  }

  fn reset_backoff(&self, node_id: i32) -> Result<()> {
    if let Some(slot) = self.map.get(&node_id) {
      slot.write().reconnect_backoff.reset();
      Ok(())
    } else {
      Err(Error::NodeNotFound)
    }
  }
}

#[derive(Debug)]
struct NodeSlot {
  config: NodeConnConfig,
  conn: Arc<NodeConn>,
  reconnect_backoff: ExponentialBackoff,
}
