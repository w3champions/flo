use bs_diesel_utils::ExecutorRef;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use diesel::prelude::*;
use std::sync::Arc;
use tonic::{Interceptor, Status};

use crate::error::Result;
use crate::schema::api_client;

#[derive(Debug, Queryable)]
pub struct ApiClient {
  id: i32,
  name: String,
  secret_key: String,
  created_at: DateTime<Utc>,
}

pub struct ApiClientStorage {
  db: ExecutorRef,
  map: DashMap<Vec<u8>, ApiClient>,
}

impl ApiClientStorage {
  pub async fn init(db: ExecutorRef) -> Result<Self> {
    let storage = ApiClientStorage {
      db: db.clone(),
      map: DashMap::new(),
    };

    let items = db
      .exec(|conn| api_client::table.load::<ApiClient>(conn))
      .await?;

    for item in items {
      storage
        .map
        .insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(storage)
  }

  pub async fn reload(&self) -> Result<()> {
    self.map.clear();

    let items = self
      .db
      .exec(|conn| api_client::table.load::<ApiClient>(conn))
      .await?;

    for item in items {
      self.map.insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(())
  }

  pub fn into_ref(self) -> ApiClientStorageRef {
    Arc::new(self)
  }

  pub fn into_interceptor(self: Arc<Self>) -> Interceptor {
    Interceptor::new(move |req| {
      let secret = req.metadata().get("x-flo-secret");
      match secret {
        Some(secret) => match self.map.get(secret.as_bytes()) {
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

pub type ApiClientStorageRef = Arc<ApiClientStorage>;
