pub mod conn;
pub mod request;

use crate::db::ExecutorRef;
use crate::error::*;
use crate::game::state::GameRegistry;
use crate::node::{Node, NodeConnConfig};
use crate::state::{Data, GetActorEntry, Reload};
use arc_swap::ArcSwap;
use conn::NodeConnActor;
use flo_state::{
  async_trait, Actor, Addr, Container, Context, Deferred, Handler, Message, RegistryRef, Service,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct NodeRegistry {
  db: ExecutorRef,
  game_reg_addr: Deferred<GameRegistry, Data>,
  map: BTreeMap<i32, Container<NodeConnActor>>,
  nodes_snapshot: ArcSwap<Vec<Node>>,
}

#[async_trait]
impl Service<Data> for NodeRegistry {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<Data>) -> Result<Self, Self::Error> {
    let game_reg_addr = registry.deferred::<GameRegistry>();
    Ok(Self {
      db: registry.data().db.clone(),
      game_reg_addr,
      map: BTreeMap::new(),
      nodes_snapshot: ArcSwap::new(Arc::new(vec![])),
    })
  }
}

#[async_trait]
impl Actor for NodeRegistry {
  async fn started(&mut self, _ctx: &mut Context<Self>) {
    if let Err(err) = self.init().await {
      tracing::error!("init: {}", err);
    }
  }
}

impl NodeRegistry {
  async fn init(&mut self) -> Result<()> {
    let game_reg_addr = self.game_reg_addr.resolve().await?;
    let nodes = self.load_snapshot().await?;

    for node in &nodes {
      tracing::debug!(node_id = node.id, "added");
      self.map.insert(
        node.id,
        NodeConnActor::new(node.into(), game_reg_addr.clone()).start(),
      );
    }

    self.nodes_snapshot.swap(Arc::new(nodes));

    Ok(())
  }

  async fn load_snapshot(&mut self) -> Result<Vec<Node>> {
    let nodes = self
      .db
      .exec(|conn| crate::node::db::get_all_nodes(conn))
      .await?;
    Ok(nodes)
  }
}

#[async_trait]
impl Handler<GetActorEntry<NodeConnActor>> for NodeRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    message: GetActorEntry<NodeConnActor>,
  ) -> Option<Addr<NodeConnActor>> {
    self.map.get(message.key()).map(|v| v.addr())
  }
}

#[async_trait]
impl Handler<Reload> for NodeRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, _: Reload) -> Result<()> {
    let nodes = self.load_snapshot().await?;

    let new_ids: Vec<i32> = nodes.iter().map(|c| c.id).collect();
    {
      for id in self.map.keys().cloned().collect::<Vec<i32>>() {
        if !new_ids.contains(&id) {
          self.map.remove(&id);
          tracing::info!(id, "node removed");
        }
      }
    }
    for node in &nodes {
      let config = NodeConnConfig::from(node);
      if !self.map.contains_key(&config.id) {
        tracing::info!(id = config.id, "node added: {}", config.addr);
        self.map.insert(
          config.id,
          NodeConnActor::new(config, self.game_reg_addr.resolve().await?).start(),
        );
      }
    }

    self.nodes_snapshot.swap(Arc::new(nodes));
    Ok(())
  }
}

pub struct ListNode;

impl Message for ListNode {
  type Result = Vec<Node>;
}

#[async_trait]
impl Handler<ListNode> for NodeRegistry {
  async fn handle(&mut self, _: &mut Context<Self>, _: ListNode) -> Vec<Node> {
    Vec::<_>::clone(&self.nodes_snapshot.load())
  }
}
