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

use flo_task::SpawnScope;

use crate::error::*;
use crate::node::NodeConnConfig;

use crate::state::event::FloControllerEventSender;
use backoff::backoff::Backoff;
use conn::{NodeConn, NodeConnEvent, NodeConnEventData};
use tracing_futures::Instrument;

pub struct NodeRegistry {
  _scope: SpawnScope,
  db: ExecutorRef,
  state: Arc<RwLock<State>>,
}

pub type NodeRegistryRef = Arc<NodeRegistry>;

impl NodeRegistry {
  pub async fn init(db: ExecutorRef, ctrl_event_sender: FloControllerEventSender) -> Result<Self> {
    let (event_sender, mut event_receiver) = channel(2);

    let scope = SpawnScope::new();

    let configs = db
      .exec(|conn| crate::node::db::get_node_conn_configs(conn))
      .await?;

    let state = Arc::new(RwLock::new(State::new(
      ctrl_event_sender,
      event_sender,
      configs,
    )?));

    tokio::spawn(
      {
        let mut scope = scope.handle();
        let state = state.clone();
        async move {
          loop {
            tokio::select! {
              _ = scope.left() => {
                break;
              }
              next = event_receiver.recv() => {
                if let Some(event) = next {
                  let node_id = event.node_id;
                  match event.data {
                    NodeConnEventData::WorkerError(err) => {
                      tracing::error!(node_id, "worker: {}", err);
                      if let Err(err) = state.read().reconnect(node_id) {
                        tracing::error!(node_id, "reconnect: {}", err);
                      }
                    }
                    NodeConnEventData::Disconnected => {
                      tracing::error!(node_id, "disconnected");
                      if let Err(err) = state.read().reconnect(node_id) {
                        tracing::error!(node_id, "reconnect: {}", err);
                      }
                    }
                    NodeConnEventData::Connected => {
                      tracing::debug!(node_id, "connected");
                      if let Err(err) = state.read().reset_backoff(node_id) {
                        tracing::error!(node_id, "reset backoff: {}", err);
                      }
                    }
                  }
                } else {
                  // will never happen because state holds a sender
                  break;
                }
              }
            };
          }
          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("event_handler_worker")),
    );

    Ok(NodeRegistry {
      db,
      _scope: scope,
      state,
    })
  }

  pub async fn reload(&self) -> Result<()> {
    let configs = self
      .db
      .exec(|conn| crate::node::db::get_node_conn_configs(conn))
      .await?;
    let mut guard = self.state.write();
    guard.update_config(configs)?;
    Ok(())
  }

  pub fn get_conn(&self, node_id: i32) -> Result<Arc<NodeConn>> {
    self.state.read().get_conn(node_id)
  }

  pub fn into_ref(self) -> NodeRegistryRef {
    Arc::new(self)
  }
}

#[derive(Debug)]
struct State {
  root_event_sender: Sender<NodeConnEvent>,
  ctrl_event_sender: FloControllerEventSender,
  map: HashMap<i32, RwLock<NodeSlot>>,
}

impl State {
  fn new(
    ctrl_event_sender: FloControllerEventSender,
    root_event_sender: Sender<NodeConnEvent>,
    configs: Vec<NodeConnConfig>,
  ) -> Result<Self> {
    let mut map = HashMap::new();
    for config in configs {
      let conn = Arc::new(NodeConn::new(
        config.id,
        &config.addr,
        &config.secret,
        root_event_sender.clone().into(),
        ctrl_event_sender.clone(),
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
            max_interval: Duration::from_secs(60),
            multiplier: 1.5,
            ..Default::default()
          },
        }),
      );
    }

    Ok(State {
      map,
      ctrl_event_sender,
      root_event_sender,
    })
  }

  fn update_config(&mut self, configs: Vec<NodeConnConfig>) -> Result<()> {
    let new_ids: Vec<i32> = configs.iter().map(|c| c.id).collect();
    {
      for id in self.map.keys().cloned().collect::<Vec<i32>>() {
        if !new_ids.contains(&id) {
          if let Some(removed) = self.map.remove(&id) {
            let addr = removed.into_inner().config.addr;
            tracing::info!(id, "node removed: {}", addr);
          }
        }
      }
    }
    for config in configs {
      if !self.map.contains_key(&config.id) {
        tracing::info!(id = config.id, "node added: {}", config.addr);
        let conn = Arc::new(NodeConn::new(
          config.id,
          &config.addr,
          &config.secret,
          self.root_event_sender.clone().into(),
          self.ctrl_event_sender.clone(),
          None,
        )?);
        self.map.insert(
          config.id,
          RwLock::new(NodeSlot {
            config,
            conn,
            reconnect_backoff: ExponentialBackoff {
              initial_interval: Duration::from_secs(5),
              current_interval: Duration::from_secs(5),
              multiplier: 1.5,
              ..Default::default()
            },
          }),
        );
      }
    }
    Ok(())
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
      let delay = guard
        .reconnect_backoff
        .next_backoff()
        .unwrap_or(guard.reconnect_backoff.max_interval);
      tracing::error!(node_id, "reconnect: backoff: {:?}", delay);
      guard.conn = Arc::new(NodeConn::new(
        node_id,
        &guard.config.addr,
        &guard.config.secret,
        self.root_event_sender.clone().into(),
        self.ctrl_event_sender.clone(),
        Some(delay),
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
