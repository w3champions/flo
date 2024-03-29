use arc_swap::ArcSwap;
use bs_diesel_utils::{DbConn, ExecutorRef};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use once_cell::sync::Lazy;
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use tonic::{metadata::MetadataValue, service::Interceptor, Request, Status};

use crate::error::*;

use crate::player::PlayerSource;
use crate::schema::{api_client, player};
use crate::state::{Data, Reload};
use flo_state::{async_trait, Actor, Context, Handler, Message, RegistryRef, Service};

pub static JWT_SECRET_BASE64: Lazy<String> =
  Lazy::new(|| env::var("JWT_SECRET_BASE64").expect("env `JWT_SECRET_BASE64`"));

#[derive(Debug, Queryable)]
pub struct ApiClient {
  id: i32,
  _name: String,
  secret_key: String,
  _created_at: DateTime<Utc>,
  player_id: i32,
}

pub struct ConfigStorage {
  db: ExecutorRef,
  api_client_map: Arc<ArcSwap<BTreeMap<Vec<u8>, ApiClient>>>,
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
      api_client_map: Arc::new(ArcSwap::new(Arc::new(map))),
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
  type Result = FloGrpcInterceptor;
}

#[async_trait]
impl Handler<GetInterceptor> for ConfigStorage {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: GetInterceptor,
  ) -> <GetInterceptor as Message>::Result {
    FloGrpcInterceptor {
      api_client_map: self.api_client_map.clone(),
    }
  }
}

pub const REQUEST_META_SECRET: &str = "x-flo-secret";
pub const REQUEST_META_API_CLIENT_ID: &str = "x-flo-api-client-id-bin";
pub const REQUEST_META_API_PLAYER_ID: &str = "x-flo-api-player-id-bin";

#[derive(Clone)]
pub struct FloGrpcInterceptor {
  api_client_map: Arc<ArcSwap<BTreeMap<Vec<u8>, ApiClient>>>,
}

impl Interceptor for FloGrpcInterceptor {
  fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
    let secret = req.metadata().get(REQUEST_META_SECRET);
    match secret {
      Some(secret) => match self.api_client_map.load().get(secret.as_bytes()) {
        Some(client) => {
          let meta = req.metadata_mut();
          meta.insert_bin(
            REQUEST_META_API_CLIENT_ID,
            MetadataValue::from_bytes(&client.id.to_le_bytes()),
          );
          meta.insert_bin(
            REQUEST_META_API_PLAYER_ID,
            MetadataValue::from_bytes(&client.player_id.to_le_bytes()),
          );
          Ok(req)
        }
        None => Err(Status::unauthenticated("invalid secret")),
      },
      None => Err(Status::unauthenticated(
        "`x-flo-secret` metadata was not found",
      )),
    }
  }
}

impl ConfigStorage {
  async fn load_map(db: &ExecutorRef) -> Result<BTreeMap<Vec<u8>, ApiClient>> {
    let mut map = BTreeMap::new();

    let (api_player_map, items) = db
      .exec(|conn| -> Result<_> {
        create_api_players(conn)?;

        let api_player_map: BTreeMap<i32, i32> = player::table
          .select((player::api_client_id, player::id))
          .filter(
            player::source
              .eq(PlayerSource::Api)
              .and(player::source_id.eq("")),
          )
          .load::<(i32, i32)>(conn)?
          .into_iter()
          .collect();

        let items = api_client::table
          .select((
            api_client::id,
            api_client::name,
            api_client::secret_key,
            api_client::created_at,
            diesel::dsl::sql::<diesel::sql_types::Integer>("0"),
          ))
          .load::<ApiClient>(conn)?;
        Ok((api_player_map, items))
      })
      .await?;

    for mut item in items {
      item.player_id = if let Some(player_id) = api_player_map.get(&item.id).cloned() {
        player_id
      } else {
        tracing::error!(id = item.id, "api player not found");
        continue;
      };
      map.insert(item.secret_key.as_bytes().to_vec(), item);
    }

    Ok(map)
  }
}

// Create API players if not exist
//
// every api client has a special player which
// - api_client_id
// - source = `PlayerSource::Api`
// - source_id = ''
fn create_api_players(conn: &DbConn) -> Result<()> {
  let sql = r#"
    insert into player(name, source, source_id, api_client_id)
    select
        c.name,
        2,
        '',
        c.id
    from api_client c
    left join player p on p.source = 2 and p.api_client_id = c.id and p.source_id = ''
    where p.id is null;
  "#;
  diesel::sql_query(sql).execute(conn)?;
  Ok(())
}

pub trait ApiRequestExt {
  fn get_api_client_id(&self) -> i32;
  fn get_api_player_id(&self) -> i32;
}

impl<T> ApiRequestExt for Request<T> {
  fn get_api_client_id(&self) -> i32 {
    let value = self
      .metadata()
      .get_bin(REQUEST_META_API_CLIENT_ID)
      .unwrap()
      .to_bytes()
      .unwrap();
    i32::from_le_bytes([value[0], value[1], value[2], value[3]])
  }

  fn get_api_player_id(&self) -> i32 {
    let value = self
      .metadata()
      .get_bin(REQUEST_META_API_PLAYER_ID)
      .unwrap()
      .to_bytes()
      .unwrap();
    i32::from_le_bytes([value[0], value[1], value[2], value[3]])
  }
}
