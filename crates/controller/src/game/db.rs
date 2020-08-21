use chrono::{DateTime, Utc};
use diesel::helper_types::Nullable;
use diesel::prelude::*;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::db::DbConn;
use crate::error::*;
use crate::game::slots::UsedSlot;
use crate::game::{
  Computer, Game, GameEntry, GameStatus, Race, Slot, SlotClientStatus, SlotSettings, SlotStatus,
  Slots,
};
use crate::map::Map;
use crate::node::{NodeRef, PlayerToken};
use crate::player::{PlayerRef, PlayerRefColumns};
use crate::schema::{game, game_used_slot};

pub fn get(conn: &DbConn, id: i32) -> Result<GameRow> {
  let row = game::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  Ok(row)
}

#[derive(Debug, Deserialize, Default, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::controller::ListGamesRequest")]
pub struct QueryGameParams {
  pub keyword: Option<String>,
  pub status: GameStatusFilter,
  pub is_private: Option<bool>,
  pub is_live: Option<bool>,
  pub take: Option<i64>,
  pub since_id: Option<i32>,
}

#[derive(Debug, S2ProtoPack)]
#[s2_grpc(message_type = "flo_grpc::controller::ListGamesReply")]
pub struct QueryGame {
  pub games: Vec<GameEntry>,
  pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, S2ProtoEnum)]
#[repr(u8)]
#[s2_grpc(proto_enum_type = "flo_grpc::controller::GameStatusFilter")]
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

pub fn get_entry(conn: &DbConn, id: i32) -> Result<GameEntry> {
  use crate::schema::player::{self};
  use diesel::dsl::sql;
  use game::dsl;

  let q = game::table.find(id).left_outer_join(player::table).select((
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
  ));

  let entry: GameEntry = q
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;

  Ok(entry)
}

pub fn query(conn: &DbConn, params: &QueryGameParams) -> Result<QueryGame> {
  use crate::schema::player::{self};
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
          .eq(GameStatus::Running)
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
#[s2_grpc(message_type = "flo_grpc::controller::CreateGameRequest")]
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

  let insert = GameInsert {
    name: &params.name,
    map_name: &meta.map.name,
    is_private: params.is_private,
    is_live: params.is_live,
    max_players: max_players as i32,
    created_by: Some(params.player_id),
    meta: meta_value,
  };

  let row = conn.transaction(|| -> Result<_> {
    let row: GameRow = diesel::insert_into(game::table)
      .values(&insert)
      .get_result(conn)?;

    upsert_used_slots(conn, row.id, slots.as_used())?;
    Ok(row)
  })?;
  Ok(row.into_game(meta, slots.into_inner())?)
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::controller::JoinGameRequest")]
pub struct JoinGameParams {
  pub game_id: i32,
  pub player_id: i32,
}

/// Adds a player into a game
pub fn join(conn: &DbConn, params: JoinGameParams) -> Result<Vec<Slot>> {
  let mut slots = get_slots(conn, params.game_id)?.slots;

  if slots.is_full() {
    return Err(Error::GameFull);
  }

  let player = crate::player::db::get_ref(conn, params.player_id)?;

  slots.join(&player);

  upsert_used_slots(conn, params.game_id, slots.as_used())?;

  Ok(slots.into_inner())
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::controller::LeaveGameRequest")]
pub struct LeaveGameParams {
  pub game_id: i32,
  pub player_id: i32,
}

#[derive(Debug)]
pub struct LeaveGame {
  pub game_ended: bool,
  pub removed_players: Vec<i32>,
  pub slots: Vec<Slot>,
}

/// Removes a player from a game
pub fn leave(conn: &DbConn, params: LeaveGameParams) -> Result<LeaveGame> {
  let GetSlots {
    mut slots,
    host_player_id,
  } = get_slots(conn, params.game_id)?;

  // host left, kick all players
  if Some(params.player_id) == host_player_id {
    let removed = slots.release_all_player_slots();
    upsert_used_slots(conn, params.game_id, slots.as_used())?;
    end_game(conn, params.game_id)?;
    Ok(LeaveGame {
      game_ended: true,
      removed_players: removed,
      slots: slots.into_inner(),
    })
  } else {
    let mut ended = false;
    let mut removed_players = Vec::with_capacity(1);
    if slots.release_player_slot(params.player_id) {
      removed_players.push(params.player_id);
      upsert_used_slots(conn, params.game_id, slots.as_used())?;
      if slots.is_empty() {
        ended = true;
        end_game(conn, params.game_id)?;
      }
    }
    Ok(LeaveGame {
      game_ended: ended,
      removed_players,
      slots: slots.into_inner(),
    })
  }
}

#[derive(Debug, Queryable)]
pub struct SlotOwnerInfo {
  pub host_player_id: Option<i32>,
  pub slot_player_id: Option<i32>,
}

impl SlotOwnerInfo {
  pub fn is_slot_owner(&self, player_id: i32) -> bool {
    self.host_player_id == Some(player_id) || self.slot_player_id == Some(player_id)
  }
}

pub fn get_slot_owner_info(conn: &DbConn, game_id: i32, slot_index: i32) -> Result<SlotOwnerInfo> {
  use game::dsl as g;
  use game_used_slot::dsl as gus;
  game::table
    .left_join(game_used_slot::table)
    .select((g::created_by, gus::player_id.nullable()))
    .filter(g::id.eq(game_id).and(gus::slot_index.eq(slot_index)))
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)
}

pub fn update_slot_settings(
  conn: &DbConn,
  game_id: i32,
  slot_index: i32,
  settings: SlotSettings,
) -> Result<Vec<Slot>> {
  let mut slots = get_slots(conn, game_id)?.slots;
  if let Some(slot) = slots.update_slot_at(slot_index, &settings) {
    sync_slot_at(conn, game_id, slot_index, &slot)?
  }
  Ok(slots.into_inner())
}

fn sync_slot_at(conn: &DbConn, game_id: i32, slot_index: i32, slot: &Slot) -> Result<()> {
  use game_used_slot::dsl;

  if slot.is_used() {
    diesel::insert_into(game_used_slot::table)
      .values(&UsedSlotInsert::from_used_slot(
        game_id,
        UsedSlot::from((slot_index as usize, slot)),
      ))
      .on_conflict((dsl::game_id, dsl::slot_index))
      .do_update()
      .set(UsedSlotUpdate::from_slot(slot))
      .execute(conn)?;
  } else {
    diesel::delete(
      game_used_slot::table.filter(dsl::game_id.eq(game_id).and(dsl::slot_index.eq(slot_index))),
    )
    .execute(conn)?;
  }

  Ok(())
}

fn upsert_used_slots(conn: &DbConn, game_id: i32, used_slots: Vec<UsedSlot>) -> Result<()> {
  use diesel::pg::expression::dsl::all;
  use diesel::pg::upsert::excluded;
  use game_used_slot::dsl;
  let indices: Vec<i32> = used_slots.iter().map(|slot| slot.slot_index).collect();
  let inserts: Vec<_> = used_slots
    .into_iter()
    .map(|slot| UsedSlotInsert::from_used_slot(game_id, slot))
    .collect();
  conn.transaction(|| {
    diesel::delete(
      game_used_slot::table.filter(
        dsl::game_id
          .eq(game_id)
          .and(dsl::slot_index.ne(all(indices))),
      ),
    )
    .execute(conn)?;
    diesel::insert_into(game_used_slot::table)
      .values(&inserts)
      .on_conflict((dsl::game_id, dsl::slot_index))
      .do_update()
      .set((
        dsl::player_id.eq(excluded(dsl::player_id)),
        dsl::team.eq(excluded(dsl::team)),
        dsl::color.eq(excluded(dsl::color)),
        dsl::computer.eq(excluded(dsl::computer)),
        dsl::handicap.eq(excluded(dsl::handicap)),
        dsl::race.eq(excluded(dsl::race)),
        dsl::client_status.eq(excluded(dsl::client_status)),
      ))
      .execute(conn)?;
    Ok(())
  })
}

#[derive(Debug)]
struct GetSlots {
  host_player_id: Option<i32>,
  slots: Slots,
}

fn get_slots(conn: &DbConn, game_id: i32) -> Result<GetSlots> {
  use crate::schema::player;
  use game_used_slot::dsl;

  let (host_player_id, max_players): (Option<i32>, i32) = {
    use game::dsl;
    game::table
      .find(game_id)
      .select((dsl::created_by, dsl::max_players))
      .first(conn)
      .optional()?
      .ok_or_else(|| Error::GameNotFound)?
  };

  let used_slots: Vec<UsedSlot> = game_used_slot::table
    .left_outer_join(player::table)
    .select(UsedSlot::columns())
    .filter(dsl::game_id.eq(game_id))
    .load(conn)?;

  let slots = Slots::from_used(max_players as usize, used_slots);
  Ok(GetSlots {
    host_player_id,
    slots,
  })
}

fn get_used_slots(conn: &DbConn, game_id: i32) -> Result<Vec<UsedSlot>> {
  use crate::schema::player;
  use game_used_slot::dsl;
  game_used_slot::table
    .left_outer_join(player::table)
    .select(UsedSlot::columns())
    .filter(dsl::game_id.eq(game_id))
    .load(conn)
    .map_err(Into::into)
}

pub fn get_full(conn: &DbConn, id: i32) -> Result<Game> {
  let row: GameRow = game::table
    .find(id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  let meta: Meta = serde_json::from_value(row.meta.clone())?;
  let used_slots = get_used_slots(conn, id)?;
  let slots: Vec<Slot> = Slots::from_used(row.max_players as usize, used_slots).into_inner();
  Ok(row.into_game(meta, slots)?)
}

pub fn get_full_and_node_token(
  conn: &DbConn,
  game_id: i32,
  player_id: i32,
) -> Result<(Game, Option<PlayerToken>)> {
  use game_used_slot::dsl as gus;
  let row: GameRow = game::table
    .find(game_id)
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::GameNotFound)?;
  let meta: Meta = serde_json::from_value(row.meta.clone())?;
  let used_slots = get_used_slots(conn, game_id)?;
  let player_token: Option<Vec<u8>> = game_used_slot::table
    .select(gus::node_token)
    .filter(gus::game_id.eq(game_id).and(gus::player_id.eq(player_id)))
    .first(conn)
    .optional()?
    .ok_or_else(|| Error::PlayerNotInGame)?;
  let max_players = row.max_players;
  Ok((
    row.into_game(
      meta,
      Slots::from_used(max_players as usize, used_slots).into_inner(),
    )?,
    player_token.and_then(|bytes| PlayerToken::from_vec(player_id, bytes)),
  ))
}

#[derive(Debug)]
pub struct GameStateFromDb {
  pub id: i32,
  pub players: Vec<i32>,
  pub node: Option<NodeRef>,
  pub created_by: Option<i32>,
}

/// Loads game players info from database
/// This is used after server restart to restore in-memory state
pub fn get_all_active_game_state(conn: &DbConn) -> Result<Vec<GameStateFromDb>> {
  use game::dsl;

  let rows: Vec<(i32, Option<Value>, Option<i32>)> = game::table
    .filter(dsl::status.eq_any(&[
      GameStatus::Preparing,
      GameStatus::Created,
      GameStatus::Running,
    ]))
    .order(dsl::created_at)
    .select((dsl::id, dsl::node, dsl::created_by))
    .load(conn)?;

  let game_ids: Vec<_> = rows.iter().map(|(id, _, _)| *id).collect();
  let mut game_players_map: HashMap<i32, Vec<i32>> = {
    use diesel::pg::expression::dsl::{all, any};
    use game_used_slot::dsl;
    let rows: Vec<(i32, Option<i32>)> = game_used_slot::table
      .select((dsl::game_id, dsl::player_id))
      .filter(
        dsl::game_id
          .eq(any(game_ids))
          .and(dsl::player_id.is_not_null())
          .and(dsl::client_status.ne(all(
            &[SlotClientStatus::Disconnected, SlotClientStatus::Left] as &[SlotClientStatus],
          ))),
      )
      .load(conn)?;
    let mut map = HashMap::new();
    for (game_id, player_id) in rows {
      if let Some(player_id) = player_id {
        map.entry(game_id).or_insert_with(|| vec![]).push(player_id)
      }
    }
    map
  };

  let mut games = Vec::with_capacity(rows.len());
  for (id, node, created_by) in rows {
    let node = node.map(serde_json::from_value).transpose()?;
    let players = game_players_map.remove(&id).unwrap_or_default();
    games.push(GameStateFromDb {
      id,
      players,
      node,
      created_by,
    });
  }
  Ok(games)
}

pub fn select_node(conn: &DbConn, id: i32, node_id: Option<i32>) -> Result<()> {
  use game::dsl;

  let node = if let Some(id) = node_id {
    let node = crate::node::db::get_node(conn, id)?;
    Some(serde_json::to_value(&crate::node::NodeRef::from(node))?)
  } else {
    None
  };

  diesel::update(game::table.find(id))
    .filter(dsl::status.eq(GameStatus::Preparing))
    .set(dsl::node.eq(node))
    .execute(conn)?;
  Ok(())
}

fn end_game(conn: &DbConn, id: i32) -> Result<()> {
  use diesel::dsl::sql;
  use game::dsl;
  conn.transaction(|| -> Result<_> {
    diesel::update(game::table.find(id))
      .filter(dsl::status.ne(GameStatus::Ended))
      .set((
        dsl::status.eq(GameStatus::Ended),
        dsl::ended_at.eq(sql("now()")),
      ))
      .execute(conn)?;
    Ok(())
  })?;
  Ok(())
}

pub fn update_created(
  conn: &DbConn,
  id: i32,
  player_tokens: HashMap<i32, PlayerToken>,
) -> Result<()> {
  use game::dsl;
  conn.transaction(|| {
    diesel::update(game::table.find(id))
      .filter(dsl::status.eq(GameStatus::Preparing))
      .set(dsl::status.eq(GameStatus::Created))
      .execute(conn)?;
    for (player_id, token) in player_tokens {
      use game_used_slot::dsl as gus;
      diesel::update(
        game_used_slot::table.filter(gus::game_id.eq(id).and(gus::player_id.eq(player_id))),
      )
      .set(gus::node_token.eq(token.as_slice()))
      .execute(conn)?;
    }
    Ok(())
  })
}

/// Created -> Preparing
pub fn update_reset_created(conn: &DbConn, id: i32) -> Result<()> {
  use game::dsl;
  use game_used_slot::dsl as gus;
  conn.transaction(|| {
    diesel::update(game::table.find(id))
      .filter(dsl::status.eq(GameStatus::Created))
      .set(dsl::status.eq(GameStatus::Preparing))
      .execute(conn)?;
    diesel::update(game_used_slot::table.filter(gus::game_id.eq(id)))
      .set(gus::node_token.eq(Option::<Vec<u8>>::None))
      .execute(conn)?;
    Ok(())
  })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
  pub map: Map,
  pub created_by: Option<PlayerRef>,
}

#[derive(Debug, Queryable)]
pub struct GameRow {
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
  pub meta: Value,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

impl GameRow {
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
pub struct GameInsert<'a> {
  pub name: &'a str,
  pub map_name: &'a str,
  pub is_private: bool,
  pub is_live: bool,
  pub max_players: i32,
  pub created_by: Option<i32>,
  pub meta: Value,
}

#[allow(unused)]
#[derive(Debug, Identifiable, Queryable, Associations)]
#[belongs_to(GameRow, foreign_key = "game_id")]
#[table_name = "game_used_slot"]
pub struct UsedSlotRow {
  pub id: i32,
  pub game_id: i32,
  pub slot_index: i32,
  pub team: i32,
  pub color: i32,
  pub computer: Computer,
  pub handicap: i32,
  pub status: SlotStatus,
  pub race: Race,
  pub client_status: SlotClientStatus,
  pub node_token: Option<Vec<u8>>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub player: Option<PlayerRef>,
}

#[allow(unused)]
type UsedSlotRowColumns = (
  game_used_slot::dsl::id,
  game_used_slot::dsl::game_id,
  game_used_slot::dsl::slot_index,
  game_used_slot::dsl::team,
  game_used_slot::dsl::color,
  game_used_slot::dsl::computer,
  game_used_slot::dsl::handicap,
  game_used_slot::dsl::status,
  game_used_slot::dsl::race,
  game_used_slot::dsl::client_status,
  game_used_slot::dsl::node_token,
  game_used_slot::dsl::created_at,
  game_used_slot::dsl::updated_at,
  Nullable<PlayerRefColumns>,
);

#[allow(unused)]
impl UsedSlotRow {
  fn columns() -> UsedSlotRowColumns {
    (
      game_used_slot::dsl::id,
      game_used_slot::dsl::game_id,
      game_used_slot::dsl::slot_index,
      game_used_slot::dsl::team,
      game_used_slot::dsl::color,
      game_used_slot::dsl::computer,
      game_used_slot::dsl::handicap,
      game_used_slot::dsl::status,
      game_used_slot::dsl::race,
      game_used_slot::dsl::client_status,
      game_used_slot::dsl::node_token,
      game_used_slot::dsl::created_at,
      game_used_slot::dsl::updated_at,
      PlayerRef::COLUMNS.nullable(),
    )
  }
}

#[derive(Debug, Insertable)]
#[table_name = "game_used_slot"]
pub struct UsedSlotInsert {
  game_id: i32,
  player_id: Option<i32>,
  slot_index: i32,
  team: i32,
  color: i32,
  computer: Computer,
  handicap: i32,
  status: SlotStatus,
  race: Race,
  client_status: SlotClientStatus,
}

impl UsedSlotInsert {
  fn from_used_slot(game_id: i32, slot: UsedSlot) -> Self {
    UsedSlotInsert {
      game_id,
      player_id: slot.player.as_ref().map(|p| p.id),
      slot_index: slot.slot_index,
      team: slot.settings.team as i32,
      color: slot.settings.color as i32,
      computer: slot.settings.computer,
      handicap: slot.settings.handicap as i32,
      status: slot.settings.status,
      race: slot.settings.race,
      client_status: slot.client_status,
    }
  }
}

#[derive(Debug, AsChangeset)]
#[table_name = "game_used_slot"]
#[changeset_options(treat_none_as_null = "true")]
pub struct UsedSlotUpdate {
  player_id: Option<i32>,
  team: i32,
  color: i32,
  computer: Computer,
  handicap: i32,
  status: SlotStatus,
  race: Race,
  client_status: SlotClientStatus,
}

impl UsedSlotUpdate {
  fn from_slot(slot: &Slot) -> Self {
    UsedSlotUpdate {
      player_id: slot.player.as_ref().map(|p| p.id),
      team: slot.settings.team as i32,
      color: slot.settings.color as i32,
      computer: slot.settings.computer,
      handicap: slot.settings.handicap as i32,
      status: slot.settings.status,
      race: slot.settings.race,
      client_status: slot.client_status,
    }
  }
}
