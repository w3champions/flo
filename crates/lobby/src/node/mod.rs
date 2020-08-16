pub mod conn;
pub mod db;
mod types;
pub use types::*;

use bs_diesel_utils::ExecutorRef;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::*;
use conn::NodeConnRef;

#[derive(Debug)]
pub struct NodesState {
  conn_map: RwLock<HashMap<i32, NodeConnRef>>,
}

pub type NodeStateRef = Arc<NodesState>;

impl NodesState {
  pub async fn init(db: ExecutorRef) -> Result<Self> {
    let configs = db
      .exec(|conn| crate::node::db::get_node_conn_configs(conn))
      .await?;

    let mut map = HashMap::new();
    for config in configs {
      map.insert(config.id, NodeConnRef::new(&config.addr, &config.secret)?);
    }

    Ok(NodesState {
      conn_map: RwLock::new(map),
    })
  }

  pub fn get_conn(&self, node_id: i32) -> Option<NodeConnRef> {
    self.conn_map.read().get(&node_id).cloned()
  }

  pub fn into_ref(self) -> NodeStateRef {
    Arc::new(self)
  }
}
