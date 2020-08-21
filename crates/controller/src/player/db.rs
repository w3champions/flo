use chrono::{DateTime, Utc};
use diesel::prelude::*;
use s2_grpc_utils::S2ProtoEnum;
use s2_grpc_utils::S2ProtoUnpack;
use serde_json::Value;
use std::convert::TryFrom;

use crate::db::DbConn;
use crate::error::*;
use crate::player::{Player, PlayerRef, PlayerSource, SourceState};
use crate::schema::player;

pub fn get(conn: &DbConn, id: i32) -> Result<Player> {
  player::table
    .find(id)
    .first::<Row>(conn)
    .optional()?
    .ok_or_else(|| Error::PlayerNotFound)
    .map(Into::into)
    .map_err(Into::into)
}

pub fn get_ref(conn: &DbConn, id: i32) -> Result<PlayerRef> {
  use player::dsl;
  player::table
    .find(id)
    .select((dsl::id, dsl::name, dsl::source, dsl::realm))
    .first::<PlayerRef>(conn)
    .optional()?
    .ok_or_else(|| Error::PlayerNotFound)
    .map_err(Into::into)
}

pub fn get_by_source_id(conn: &DbConn, source_id: &str) -> Result<Option<Player>> {
  use player::dsl;
  player::table
    .filter(dsl::source_id.eq(source_id))
    .first::<Row>(conn)
    .optional()
    .map(|p| p.map(Into::into))
    .map_err(Into::into)
}

pub fn get_refs_by_ids(conn: &DbConn, ids: &[i32]) -> Result<Vec<PlayerRef>> {
  use player::dsl;
  player::table
    .filter(dsl::id.eq_any(ids))
    .select((dsl::id, dsl::name, dsl::source, dsl::realm))
    .load(conn)
    .map_err(Into::into)
}

#[derive(Debug, Insertable)]
#[table_name = "player"]
pub struct UpsertPlayer {
  pub name: String,
  pub source: PlayerSource,
  pub source_id: String,
  pub source_state: Option<Value>,
  pub realm: Option<String>,
}

impl TryFrom<flo_grpc::controller::UpdateAndGetPlayerRequest> for UpsertPlayer {
  type Error = Error;

  fn try_from(value: flo_grpc::controller::UpdateAndGetPlayerRequest) -> Result<Self, Self::Error> {
    Ok(UpsertPlayer {
      source: PlayerSource::unpack_enum(value.source()),
      name: value.name,
      source_id: value.source_id,
      source_state: {
        let state = value
          .source_state
          .map(|state| SourceState::unpack(state))
          .transpose()?;
        Some(serde_json::to_value(&state)?)
      },
      realm: S2ProtoUnpack::unpack(value.realm)?,
    })
  }
}

pub fn upsert(conn: &DbConn, data: &UpsertPlayer) -> Result<Player> {
  use player::dsl;

  diesel::insert_into(player::table)
    .values(data)
    .on_conflict((dsl::source, dsl::source_id))
    .do_update()
    .set(Update {
      name: &data.name,
      source_state: data.source_state.as_ref(),
      realm: data.realm.as_ref().map(AsRef::as_ref),
    })
    .get_result::<Row>(conn)
    .map(Into::into)
    .map_err(Into::into)
}

#[derive(Debug, Insertable)]
#[table_name = "player"]
struct Insert<'a> {
  name: &'a str,
  source: PlayerSource,
  source_id: &'a str,
  source_state: Option<Value>,
  realm: Option<&'a str>,
}

#[derive(Debug, AsChangeset)]
#[table_name = "player"]
#[changeset_options(treat_none_as_null = "true")]
struct Update<'a> {
  name: &'a str,
  source_state: Option<&'a Value>,
  realm: Option<&'a str>,
}

#[derive(Debug, Queryable)]
pub struct Row {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub source_id: String,
  pub source_state: Option<Value>,
  pub realm: Option<String>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

impl From<Row> for Player {
  fn from(row: Row) -> Player {
    Player {
      id: row.id,
      name: row.name,
      source: row.source,
      source_id: row.source_id,
      source_state: row
        .source_state
        .map(|value| serde_json::from_value(value))
        .transpose()
        .unwrap_or(Some(SourceState::Invalid)),
      realm: row.realm,
      created_at: row.created_at,
      updated_at: row.updated_at,
    }
  }
}

impl From<Row> for PlayerRef {
  fn from(p: Row) -> Self {
    PlayerRef {
      id: p.id,
      name: p.name,
      source: p.source,
      realm: p.realm,
    }
  }
}
