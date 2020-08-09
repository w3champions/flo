use bs_diesel_utils::ExecutorRef;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use diesel::prelude::*;
use parking_lot::RwLock;
use std::sync::Arc;
use tonic::{Interceptor, Status};

use crate::error::Result;
use crate::node::Node;
use crate::schema::api_client;

#[derive(Debug, Queryable)]
pub struct ApiClient {
  id: i32,
  name: String,
  secret_key: String,
  created_at: DateTime<Utc>,
}

pub struct ConfigStorage {
  db: ExecutorRef,
  api_client_map: DashMap<Vec<u8>, ApiClient>,
  nodes: RwLock<Vec<Node>>,
}

impl ConfigStorage {
  pub async fn init(db: ExecutorRef) -> Result<Self> {
    let nodes = db.exec(|conn| crate::node::db::get_all_nodes(conn)).await?;

    let storage = ConfigStorage {
      db: db.clone(),
      api_client_map: DashMap::new(),
      nodes: RwLock::new(nodes),
    };

    let items = db
      .exec(|conn| api_client::table.load::<ApiClient>(conn))
      .await?;

    for item in items {
      storage
        .api_client_map
        .insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(storage)
  }

  pub fn with_nodes<F, R>(&self, f: F) -> R
  where
    F: FnOnce(&[Node]) -> R,
  {
    let guard = self.nodes.read();
    f(&guard)
  }

  pub async fn reload(&self) -> Result<()> {
    let nodes = self
      .db
      .exec(|conn| crate::node::db::get_all_nodes(conn))
      .await?;

    *self.nodes.write() = nodes;

    self.api_client_map.clear();

    let items = self
      .db
      .exec(|conn| api_client::table.load::<ApiClient>(conn))
      .await?;

    for item in items {
      self
        .api_client_map
        .insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(())
  }

  pub fn into_ref(self) -> ConfigClientStorageRef {
    Arc::new(self)
  }

  pub fn into_interceptor(self: Arc<Self>) -> Interceptor {
    Interceptor::new(move |req| {
      let secret = req.metadata().get("x-flo-secret");
      match secret {
        Some(secret) => match self.api_client_map.get(secret.as_bytes()) {
          Some(_) => Ok(req),
          None => Err(Status::unauthenticated("invalid secret")),
        },
        None => Err(Status::unauthenticated(
          "`x-flo-secret` metadata was not found",
        )),
      }
    })
  }
}

pub type ConfigClientStorageRef = Arc<ConfigStorage>;
