use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::Value;

use crate::db::DbConn;
use crate::error::*;
use crate::player::PlayerSource;
use crate::schema::player;

pub fn get(conn: &DbConn, id: i32) -> Result<Row> {
  use player::dsl;
  player::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::PlayerNotFound)
    .map_err(Into::into)
}

pub fn get_by_source_id(conn: &DbConn, source_id: &str) -> Result<Option<Row>> {
  use player::dsl;
  player::table
    .filter(dsl::source_id.eq(source_id))
    .first(conn)
    .optional()
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

pub fn upsert(conn: &DbConn, data: &UpsertPlayer) -> Result<Row> {
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
    .get_result(conn)
    .map_err(Into::into)
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
