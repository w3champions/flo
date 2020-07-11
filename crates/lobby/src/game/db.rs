use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::DbConn;
use crate::error::*;
use crate::game::{GameStatus, Slot};
use crate::map::Map;
use crate::schema::game;

#[derive(Debug, Queryable)]
pub struct Row {
  pub id: i32,
  pub name: String,
  pub map_name: String,
  pub status: GameStatus,
  pub node: Option<Value>,
  pub is_private: bool,
  pub secret: Option<i32>,
  pub is_live: bool,
  pub num_players: i32,
  pub max_players: i32,
  pub created_by: i32,
  pub started_at: Option<DateTime<Utc>>,
  pub ended_at: Option<DateTime<Utc>>,
  pub meta: Value,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

pub fn get(conn: &DbConn, id: i32) -> Result<Row> {
  let row = game::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  Ok(row)
}

#[derive(Debug, Deserialize, Default)]
pub struct QueryGameParams {
  pub keyword: Option<String>,
  pub status: Option<GameStatus>,
  pub is_private: Option<bool>,
  pub is_live: Option<bool>,
}

pub struct QueryGame {
  pub rows: Vec<Row>,
  pub has_more: bool,
}

pub fn query(
  conn: &DbConn,
  params: &QueryGameParams,
  take: Option<i64>,
  since_id: Option<i32>,
) -> Result<QueryGame> {
  use game::dsl;

  let take = std::cmp::min(100, take.unwrap_or(30));

  let mut q = game::table.limit(take + 1).into_boxed();

  if let Some(ref keyword) = params.keyword {
    let like = format!("%{}%", keyword.trim());
    q = q.filter(dsl::name.ilike(like.clone()).or(dsl::map_name.ilike(like)));
  }

  if let Some(status) = params.status {
    q = q.filter(dsl::status.eq(status));
  }

  if let Some(is_private) = params.is_private.clone() {
    q = q.filter(dsl::is_private.eq(is_private));
  } else {
    q = q.filter(dsl::is_private.eq(false));
  }

  if let Some(is_live) = params.is_live.clone() {
    q = q.filter(dsl::is_live.eq(is_live));
  }

  if let Some(id) = since_id {
    q = q.filter(dsl::id.gt(id))
  }

  let mut rows: Vec<Row> = q.load(conn)?;
  let has_more = rows.len() > take as usize;
  if has_more {
    rows.truncate(take as usize);
  }

  Ok(QueryGame { rows, has_more })
}

pub fn delete(conn: &DbConn, game_id: i32, created_by: Option<i32>) -> Result<()> {
  use game::dsl;

  let mut q = game::table.find(game_id).into_boxed();
  if let Some(created_by) = created_by {
    q = q.filter(dsl::created_by.eq(created_by));
  }
  let status: GameStatus = q
    .select(dsl::status)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  if status != GameStatus::Waiting {
    return Err(Error::GameNotDeletable);
  }
  diesel::delete(game::table.find(game_id)).execute(conn)?;
  Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateGameParams {
  pub name: String,
  pub map: Map,
  pub is_private: bool,
  pub is_live: bool,
}

pub fn create(conn: &DbConn, params: CreateGameParams, created_by: Option<i32>) -> Result<Row> {
  let num_players = 1;
  let max_players = params.map.players.len() as i32;

  // let slots = Vec::with_capacity(24);
  // for i in 0..24 {
  //   slots.push(Slot {
  //
  //   });
  // }
  //
  // let meta = Meta {
  //   map: params.map,
  // };
  //
  // let insert = Insert {
  //   name: &params.name,
  //   map_name: &params.map.name,
  // }
  unimplemented!()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
  pub map: Map,
  pub slots: Vec<Slot>,
}

#[derive(Debug, Insertable)]
#[table_name = "game"]
pub struct Insert<'a> {
  pub name: &'a str,
  pub map_name: &'a str,
  pub is_private: bool,
  pub is_live: bool,
  pub num_players: i32,
  pub max_players: i32,
  pub created_by: i32,
  pub meta: Value,
}

#[derive(Debug, AsChangeset)]
#[table_name = "game"]
#[changeset_options(treat_none_as_null = "true")]
pub struct Update<'a> {
  pub name: &'a str,
  pub map_name: &'a str,
  pub meta: Value,
}
