use chrono::{DateTime, Utc};
use diesel::prelude::*;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::DbConn;
use crate::error::*;
use crate::game::{
  Computer, Game, GameEntry, GameStatus, Race, Slot, SlotSettings, SlotStatus, Slots,
};
use crate::map::Map;
use crate::player::PlayerRef;
use crate::schema::game;
use crate::state::LockedGameState;

pub fn get(conn: &DbConn, id: i32) -> Result<Row> {
  let row = game::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  Ok(row)
}

#[derive(Debug, Deserialize, Default, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::lobby::ListGamesRequest")]
pub struct QueryGameParams {
  pub keyword: Option<String>,
  pub status: GameStatusFilter,
  pub is_private: Option<bool>,
  pub is_live: Option<bool>,
  pub take: Option<i64>,
  pub since_id: Option<i32>,
}

#[derive(Debug, S2ProtoPack)]
#[s2_grpc(message_type = "flo_grpc::lobby::ListGamesReply")]
pub struct QueryGame {
  pub games: Vec<GameEntry>,
  pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, S2ProtoEnum)]
#[repr(u8)]
#[s2_grpc(proto_enum_type = "flo_grpc::lobby::GameStatusFilter")]
pub enum GameStatusFilter {
  All = 0,
  Open = 1,
  Live = 2,
  Ended = 3,
}

impl Default for GameStatusFilter {
  fn default() -> Self {
    Self::All
  }
}

pub fn query(conn: &DbConn, params: &QueryGameParams) -> Result<QueryGame> {
  use crate::schema::player::{self, dsl as player_dsl};
  use diesel::dsl::sql;
  use game::dsl;

  let take = std::cmp::min(100, params.take.clone().unwrap_or(30));

  let mut q = game::table
    .left_outer_join(player::table)
    .select((
      dsl::id,
      dsl::name,
      dsl::map_name,
      dsl::status,
      dsl::is_private,
      dsl::is_live,
      sql::<diesel::sql_types::Integer>("0"),
      dsl::max_players,
      dsl::started_at,
      dsl::ended_at,
      dsl::created_at,
      dsl::updated_at,
      PlayerRef::COLUMNS.nullable(),
    ))
    .limit(take + 1)
    .into_boxed();

  if let Some(ref keyword) = params.keyword {
    let like = format!("%{}%", keyword.trim());
    q = q.filter(dsl::name.ilike(like.clone()).or(dsl::map_name.ilike(like)));
  }

  match params.status {
    GameStatusFilter::All => q = q.filter(dsl::status.ne(GameStatus::Ended)),
    GameStatusFilter::Open => q = q.filter(dsl::status.eq(GameStatus::Preparing)),
    GameStatusFilter::Live => {
      q = q.filter(
        dsl::status
          .eq(GameStatus::Playing)
          .and(dsl::is_private.eq(false)),
      )
    }
    GameStatusFilter::Ended => q = q.filter(dsl::status.eq(GameStatus::Ended)),
  }

  if let Some(is_private) = params.is_private.clone() {
    q = q.filter(dsl::is_private.eq(is_private));
  } else {
    q = q.filter(dsl::is_private.eq(false));
  }

  if let Some(is_live) = params.is_live.clone() {
    q = q.filter(dsl::is_live.eq(is_live));
  }

  if let Some(id) = params.since_id.clone() {
    q = q.filter(dsl::id.gt(id))
  }

  let mut games: Vec<GameEntry> = q.load(conn)?;

  let has_more = games.len() > take as usize;
  if has_more {
    games.truncate(take as usize);
  }

  Ok(QueryGame { games, has_more })
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
  if status != GameStatus::Preparing {
    return Err(Error::GameNotDeletable);
  }
  diesel::delete(game::table.find(game_id)).execute(conn)?;
  Ok(())
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::lobby::CreateGameRequest")]
pub struct CreateGameParams {
  pub player_id: i32,
  pub name: String,
  pub map: Map,
  pub is_private: bool,
  pub is_live: bool,
}

/// Creates a game, make the creator as the first player
pub fn create(conn: &DbConn, params: CreateGameParams) -> Result<Game> {
  let max_players = params.map.players.len();

  if max_players == 0 {
    return Err(Error::MapHasNoPlayer);
  }

  let player = crate::player::db::get_ref(conn, params.player_id)?;
  let mut slots = Slots::new(max_players);
  slots.join(&player);

  let meta = Meta {
    map: params.map,
    created_by: player.into(),
  };

  let meta_value = serde_json::to_value(&meta)?;

  let insert = Insert {
    name: &params.name,
    map_name: &meta.map.name,
    is_private: params.is_private,
    is_live: params.is_live,
    max_players: max_players as i32,
    created_by: Some(params.player_id),
    slots: serde_json::to_value(&slots as &[Slot])?,
    meta: meta_value,
  };

  let row: Row = diesel::insert_into(game::table)
    .values(&insert)
    .get_result(conn)?;

  Ok(row.into_game(meta, slots.into_inner())?)
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::lobby::JoinGameRequest")]
pub struct JoinGameParams {
  pub game_id: i32,
  pub player_id: i32,
}

/// Adds a player into a game
pub fn join(conn: &DbConn, params: JoinGameParams) -> Result<Vec<Slot>> {
  let mut slots = get_slots(conn, params.game_id)?;

  if slots.is_full() {
    return Err(Error::GameFull);
  }

  let player = crate::player::db::get_ref(conn, params.player_id)?;

  slots.join(&player);
  update_slots(conn, params.game_id, &slots)?;

  Ok(slots.into_inner())
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::lobby::LeaveGameRequest")]
pub struct LeaveGameParams {
  pub game_id: i32,
  pub player_id: i32,
}

/// Removes a player from a game
pub fn leave(conn: &DbConn, params: LeaveGameParams) -> Result<Vec<Slot>> {
  let mut slots = get_slots(conn, params.game_id)?;
  if slots.release_player_slot(params.player_id) {
    if slots.is_empty() {
      end_game(conn, params.game_id)?;
    } else {
      update_slots(conn, params.game_id, &slots)?;
    }
  }
  Ok(slots.into_inner())
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::lobby::UpdateGameSlotSettingsRequest")]
pub struct UpdateGameSlotSettingsParams {
  pub game_id: i32,
  pub player_id: i32,
  pub settings: SlotSettings,
}

pub fn update_slot_settings(
  conn: &DbConn,
  params: UpdateGameSlotSettingsParams,
) -> Result<Vec<Slot>> {
  let mut slots = get_slots(conn, params.game_id)?;
  if slots.update_player_slot(params.player_id, &params.settings) {
    update_slots(conn, params.game_id, &slots)?
  }
  Ok(slots.into_inner())
}

pub fn get_full(conn: &DbConn, id: i32) -> Result<Game> {
  let row: Row = game::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  let meta: Meta = serde_json::from_value(row.meta.clone())?;
  let slots: Vec<Slot> = serde_json::from_value(row.slots.clone())?;
  Ok(row.into_game(meta, slots)?)
}

#[derive(Debug)]
pub struct GameStateFromDb {
  pub id: i32,
  pub players: Vec<i32>,
}

/// Loads game players info from database
/// This is used after server restart to restore in-memory state
pub fn get_all_active_game_state(conn: &DbConn) -> Result<Vec<GameStateFromDb>> {
  use game::dsl;

  let rows: Vec<(i32, Value)> = game::table
    .filter(dsl::status.eq_any(&[GameStatus::Preparing, GameStatus::Playing]))
    .select((dsl::id, dsl::slots))
    .load(conn)?;
  let mut games = Vec::with_capacity(rows.len());
  for (id, slots) in rows {
    let slots: Vec<Slot> = serde_json::from_value(slots)?;
    games.push(GameStateFromDb {
      id,
      players: slots
        .into_iter()
        .filter_map(|s| s.player.map(|p| p.id))
        .collect(),
    });
  }
  Ok(games)
}

fn get_slots(conn: &DbConn, id: i32) -> Result<Slots> {
  use game::dsl;
  let value: Value = game::table
    .select(dsl::slots)
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  Ok(Slots::from_vec(serde_json::from_value(value)?))
}

fn update_slots(conn: &DbConn, id: i32, slots: &[Slot]) -> Result<()> {
  use game::dsl;
  diesel::update(game::table.find(id))
    .set(dsl::slots.eq(serde_json::to_value(slots)?))
    .execute(conn)?;
  Ok(())
}

fn end_game(conn: &DbConn, id: i32) -> Result<()> {
  use diesel::dsl::sql;
  use game::dsl;
  diesel::update(game::table.find(id))
    .set((
      dsl::status.eq(GameStatus::Ended),
      dsl::ended_at.eq(sql("now()")),
    ))
    .execute(conn)?;
  Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
  pub map: Map,
  pub created_by: Option<PlayerRef>,
}

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
  pub max_players: i32,
  pub created_by: Option<i32>,
  pub started_at: Option<DateTime<Utc>>,
  pub ended_at: Option<DateTime<Utc>>,
  pub slots: Value,
  pub meta: Value,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

impl Row {
  pub(crate) fn into_game(self, meta: Meta, slots: Vec<Slot>) -> Result<Game> {
    let num_players = slots.iter().filter(|s| s.player.is_some()).count() as i32;
    Ok(Game {
      id: self.id,
      name: self.name,
      status: self.status,
      map: meta.map,
      slots,
      node: self.node.map(serde_json::from_value).transpose()?,
      is_private: self.is_private,
      secret: self.secret,
      is_live: self.is_live,
      num_players,
      max_players: self.max_players,
      created_by: meta.created_by,
      started_at: self.started_at,
      ended_at: self.ended_at,
      created_at: self.created_at,
      updated_at: self.updated_at,
    })
  }
}

#[derive(Debug, Insertable)]
#[table_name = "game"]
pub struct Insert<'a> {
  pub name: &'a str,
  pub map_name: &'a str,
  pub is_private: bool,
  pub is_live: bool,
  pub max_players: i32,
  pub created_by: Option<i32>,
  pub slots: Value,
  pub meta: Value,
}
