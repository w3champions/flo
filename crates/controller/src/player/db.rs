use crate::db::DbConn;
use crate::error::*;
use crate::player::{Player, PlayerRef, PlayerSource, SourceState};
use crate::schema::{player, player_mute};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

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

pub fn get_refs_by_ids(conn: &DbConn, ids: &[i32]) -> Result<Vec<PlayerRef>> {
  use player::dsl;
  player::table
    .filter(dsl::id.eq_any(ids))
    .select((dsl::id, dsl::name, dsl::source, dsl::realm))
    .load(conn)
    .map_err(Into::into)
}

pub fn get_client_refs_by_ids(
  conn: &DbConn,
  api_client_id: i32,
  ids: &[i32],
) -> Result<Vec<PlayerRef>> {
  use player::dsl;
  player::table
    .filter(dsl::api_client_id.eq(api_client_id))
    .filter(dsl::id.eq_any(ids))
    .select((dsl::id, dsl::name, dsl::source, dsl::realm))
    .load(conn)
    .map_err(Into::into)
}

pub fn get_player_map_by_api_source_ids(
  conn: &DbConn,
  api_client_id: i32,
  ids: Vec<String>,
) -> Result<HashMap<String, PlayerRef>> {
  use player::dsl;
  let pairs = player::table
    .filter(
      dsl::source
        .eq(PlayerSource::Api)
        .and(dsl::api_client_id.eq(api_client_id)),
    )
    .filter(dsl::source_id.eq_any(ids))
    .select((
      dsl::source_id,
      (dsl::id, dsl::name, dsl::source, dsl::realm),
    ))
    .load::<(String, PlayerRef)>(conn)?;
  Ok(pairs.into_iter().collect())
}

#[derive(Debug, Insertable)]
#[table_name = "player"]
pub struct UpsertPlayer {
  pub api_client_id: i32,
  pub name: String,
  pub source: PlayerSource,
  pub source_id: String,
  pub source_state: Option<Value>,
  pub realm: Option<String>,
}

pub fn upsert(conn: &DbConn, data: &UpsertPlayer) -> Result<Player> {
  use player::dsl;

  if data.source_id.is_empty() {
    return Err(Error::PlayerSourceIdInvalid);
  }

  diesel::insert_into(player::table)
    .values(data)
    .on_conflict((dsl::api_client_id, dsl::source, dsl::source_id))
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

pub fn add_mute(conn: &DbConn, player_id: i32, mute_player_id: i32) -> Result<()> {
  #[derive(Insertable)]
  #[table_name = "player_mute"]
  struct Insert {
    player_id: i32,
    mute_player_id: i32,
  }

  diesel::insert_into(player_mute::table)
    .values(&Insert {
      player_id,
      mute_player_id,
    })
    .on_conflict((player_mute::player_id, player_mute::mute_player_id))
    .do_nothing()
    .execute(conn)?;

  Ok(())
}

pub fn remove_mute(conn: &DbConn, player_id: i32, mute_player_id: i32) -> Result<()> {
  diesel::delete(
    player_mute::table.filter(
      player_mute::player_id
        .eq(player_id)
        .and(player_mute::mute_player_id.eq(mute_player_id)),
    ),
  )
  .execute(conn)?;

  Ok(())
}

pub fn get_mute_list_map(conn: &DbConn, player_ids: &[i32]) -> Result<BTreeMap<i32, Vec<i32>>> {
  use diesel::pg::expression::dsl::any;
  let pairs: Vec<(i32, i32)> = player_mute::table
    .select((player_mute::player_id, player_mute::mute_player_id))
    .filter(
      player_mute::player_id
        .eq(any(player_ids))
        .and(player_mute::mute_player_id.eq(any(player_ids))),
    )
    .load(conn)?;
  let mut map = BTreeMap::new();
  for (player_id, mute_player_id) in pairs {
    map
      .entry(player_id)
      .or_insert_with(|| vec![])
      .push(mute_player_id);
  }
  Ok(map)
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
  pub api_client_id: i32,
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
