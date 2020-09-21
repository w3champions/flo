use arc_swap::ArcSwap;
use bs_diesel_utils::ExecutorRef;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use lazy_static::lazy_static;
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use tonic::{Interceptor, Status};

use crate::error::*;

use crate::schema::api_client;
use crate::state::{Data, Reload};
use flo_state::{async_trait, Actor, Context, Handler, Message, RegistryRef, Service};

lazy_static! {
  pub static ref JWT_SECRET_BASE64: String =
    env::var("JWT_SECRET_BASE64").expect("env `JWT_SECRET_BASE64`");
}

#[derive(Debug, Queryable)]
pub struct ApiClient {
  id: i32,
  name: String,
  secret_key: String,
  created_at: DateTime<Utc>,
}

pub struct ConfigStorage {
  db: ExecutorRef,
  api_client_map: ArcSwap<BTreeMap<Vec<u8>, ApiClient>>,
}

impl Actor for ConfigStorage {}

#[async_trait]
impl Service<Data> for ConfigStorage {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<Data>) -> Result<Self, Self::Error> {
    let db = registry.data().db.clone();
    let map = ConfigStorage::load_map(&db).await?;

    let storage = ConfigStorage {
      db,
      api_client_map: ArcSwap::new(Arc::new(map)),
    };

    Ok(storage)
  }
}

#[async_trait]
impl Handler<Reload> for ConfigStorage {
  async fn handle(&mut self, _: &mut Context<Self>, _: Reload) -> <Reload as Message>::Result {
    let map = Self::load_map(&self.db).await?;
    self.api_client_map.swap(Arc::new(map));
    Ok(())
  }
}

pub struct GetInterceptor;
impl Message for GetInterceptor {
  type Result = Interceptor;
}

#[async_trait]
impl Handler<GetInterceptor> for ConfigStorage {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetInterceptor,
  ) -> <GetInterceptor as Message>::Result {
    self.as_interceptor()
  }
}

impl ConfigStorage {
  pub fn as_interceptor(&self) -> Interceptor {
    let map = self.api_client_map.clone();
    Interceptor::new(move |req| {
      let secret = req.metadata().get("x-flo-secret");
      match secret {
        Some(secret) => match map.load().get(secret.as_bytes()) {
          Some(_) => Ok(req),
          None => Err(Status::unauthenticated("invalid secret")),
        },
        None => Err(Status::unauthenticated(
          "`x-flo-secret` metadata was not found",
        )),
      }
    })
  }

  async fn load_map(db: &ExecutorRef) -> Result<BTreeMap<Vec<u8>, ApiClient>> {
    let mut map = BTreeMap::new();

    let items = db
      .exec(|conn| api_client::table.load::<ApiClient>(conn))
      .await?;

    for item in items {
      map.insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(map)
  }
}
