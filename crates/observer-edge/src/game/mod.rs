pub mod snapshot;
pub mod event;
pub mod stats;

use std::time::Duration;
use chrono::{DateTime, Utc, TimeZone};
use flo_kinesis::iterator::GameChunk;
use flo_observer::record::{GameRecordData, RTTStats};
use flo_w3gs::action::PlayerAction;
use flo_w3gs::protocol::constants::PacketTypeId;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use tracing::{Span};
use async_graphql::{SimpleObject, Enum};
use crate::error::{Result, Error};
use crate::services::Services;
use self::stats::{GameStats, ActionStats};
use self::snapshot::{GameSnapshot, GameSnapshotMap, GameSnapshotWithStats};

pub struct GameHandler {
  _services: Services,
  meta: GameMeta,
  game: FetchGameState,
  next_record_id: u32,
  last_arrival_timestamp: Option<f64>,
  records: Vec<GameRecordData>,
  span: Span,
}

impl GameHandler {
  pub fn new(services: Services, game_id: i32, records: GameChunk) -> Self {
    let span = tracing::info_span!("game", game_id);
    let meta = GameMeta {
      id: game_id,
      started_at: Utc.timestamp_millis((records.approximate_arrival_timestamp * 1000.) as i64),
      ended_at: None,
      duration: None,
    };

    span.in_scope(|| {
      tracing::info!("started at: {}", meta.started_at);
    });

    Self {
      _services: services,
      meta,
      game: FetchGameState::new(),
      next_record_id: records.max_seq_id + 1,
      last_arrival_timestamp: None,
      records: records.records,
      span,
    }
  }

  pub fn id(&self) -> i32 {
    self.meta.id
  }

  pub fn set_fetch_result(&mut self, result: Result<Game>, snapshot_map: &mut GameSnapshotMap) {
    match result {
      Ok(game) => {
        let game_id = game.id;
        let mut stats = GameStats::new(&game);

        if let FetchGameState::Loading { ref mut deferred} = self.game {
          for item in std::mem::replace(deferred, vec![]) {
            match item {
              DeferredOp::PushAction(time_increment_ms, actions) => {
                if let Some(item) = stats.put_actions(time_increment_ms, &actions) {
                  snapshot_map.insert_game_action_stats(game_id, item);
                }
              },
              DeferredOp::PushRTTStats(item) => {
                snapshot_map.insert_game_rtt_stats(game_id, stats.put_rtt(item));
              },
            }
          }
        }

        self.game = FetchGameState::Loaded {
          game,
          stats
        };
      },
      Err(err) => {
        self.game = FetchGameState::Failed(err);
      },
    }
  }

  pub fn make_snapshot(&self) -> Result<GameSnapshot> {
    let game = self.game.get()?;
    Ok(GameSnapshot::new(&self.meta, game))
  }

  pub fn make_snapshot_with_stats(&self) -> Result<GameSnapshotWithStats> {
    match self.game {
      FetchGameState::Loading { .. } => Err(Error::GameNotReady("still loading".to_string())),
      FetchGameState::Loaded { ref game, ref stats } => Ok(GameSnapshotWithStats {
        game: GameSnapshot::new(&self.meta, game),
        stats: stats.make_snapshot(),
      }),
      FetchGameState::Failed(ref e) => Err(Error::GameNotReady(e.to_string())),
    }
  }

  pub fn handle_chunk(&mut self, chunk: GameChunk, snapshot_map: &mut GameSnapshotMap) -> Result<()> {
    if chunk.min_seq_id != self.next_record_id {
      if is_delayed_game_end_record(&chunk) {
        self.handle_records(chunk.approximate_arrival_timestamp, chunk.records, snapshot_map)?;
      } else {
        self.span.in_scope(|| {
          tracing::debug!("{:?}", chunk);
        });
        return Err(Error::UnexpectedGameRecords {
          expected: self.next_record_id,
          range: [chunk.min_seq_id, chunk.max_seq_id],
          len: chunk.records.len(),
        })
      }
    } else {
      self.next_record_id = chunk.max_seq_id + 1;
      self.handle_records(chunk.approximate_arrival_timestamp, chunk.records, snapshot_map)?;
    }
    self.last_arrival_timestamp = Some(chunk.approximate_arrival_timestamp);
    Ok(())
  }

  fn handle_records(&mut self, t: f64, records: Vec<GameRecordData>, snapshot_map: &mut GameSnapshotMap) -> Result<()> {
    for record in records {
      match record {
        GameRecordData::W3GS(ref packet) => {
          match packet.type_id() {
            PacketTypeId::IncomingAction | PacketTypeId::IncomingAction2 => {
              use flo_w3gs::protocol::action::TimeSlot;
              let payload: TimeSlot =  packet.decode_payload_bytes()?;
              self.game.put_actions(self.meta.id, payload.time_increment_ms, &payload.actions, snapshot_map)?;
            },
            _ => {}
          }
        },
        GameRecordData::StartLag(_) => {},
        GameRecordData::StopLag(_) => {},
        GameRecordData::GameEnd => {
          let ended_at = Utc.timestamp_millis((t * 1000.) as i64);
          let duration = ended_at.signed_duration_since(self.meta.started_at);
          self.meta.ended_at.replace(ended_at);
          self.meta.duration = duration.to_std().ok();
          self.span.in_scope(|| {
            tracing::info!("ended at: {:?}, duration: {}", ended_at, duration);
          });
          snapshot_map.end_game(&self.meta);
        },
        GameRecordData::TickChecksum { .. } => {},
        GameRecordData::RTTStats(stats) => {
          self.game.put_rtt(self.meta.id, stats, snapshot_map);
          return Ok(())
        },
      }
      self.records.push(record);
    }
    Ok(())
  }
}

// There was bug causes the `GameEnd` records have record id 0
fn is_delayed_game_end_record(records: &GameChunk) -> bool {
  if records.min_seq_id == 0 && records.records.len() == 1 {
    if let Some(&GameRecordData::GameEnd) = records.records.first() {
      return true
    }
  }
  false
}

pub struct GameMeta {
  pub id: i32,
  pub started_at: DateTime<Utc>,
  pub ended_at: Option<DateTime<Utc>>,
  pub duration: Option<Duration>,
}

enum FetchGameState {
  Loading {
    deferred: Vec<DeferredOp>,
  },
  Loaded {
    game: Game,
    stats: GameStats,
  },
  Failed(Error),
}

impl FetchGameState {
  fn new() -> Self {
    Self::Loading {
      deferred: vec![]
    }
  }

  fn get(&self) -> Result<&Game> {
    match self {
      FetchGameState::Loading { .. } => Err(Error::GameNotReady("still loading".to_string())),
      FetchGameState::Loaded { ref game, .. } => Ok(game),
      FetchGameState::Failed(ref e) => Err(Error::GameNotReady(e.to_string())),
    }
  }

  fn put_actions(&mut self, id: i32, time_increment_ms: u16, actions: &[PlayerAction], snapshot_map: &mut GameSnapshotMap) -> Result<()>
  {
    match self {
      FetchGameState::Loading { ref mut deferred } => {
        deferred.push(DeferredOp::PushAction(time_increment_ms, actions.to_vec()));
        Ok(())
      },
      FetchGameState::Loaded {ref mut stats, .. } => {
        if let Some(stats) = stats.put_actions(time_increment_ms, actions) {
          snapshot_map.insert_game_action_stats(id, stats);
        }
        Ok(())
      },
      FetchGameState::Failed(ref e) => Err(Error::GameNotReady(e.to_string())),
    }
  }

  fn put_rtt(&mut self, id: i32, item: RTTStats, snapshot_map: &mut GameSnapshotMap) -> Result<()>
  {
    match self {
      FetchGameState::Loading { ref mut deferred } => {
        deferred.push(DeferredOp::PushRTTStats(item));
        Ok(())
      },
      FetchGameState::Loaded {ref mut stats, .. } => {
        snapshot_map.insert_game_rtt_stats(id, stats.put_rtt(item));
        Ok(())
      },
      FetchGameState::Failed(ref e) => Err(Error::GameNotReady(e.to_string())),
    }
  }
}

enum DeferredOp {
  PushAction(u16, Vec<PlayerAction>),
  PushRTTStats(RTTStats),
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::Game")]
pub struct Game {
  pub id: i32,
  pub name: String,
  pub map: Map,
  pub node: Node,
  pub slots: Vec<Slot>,
  pub random_seed: i32,
  pub game_version: Option<String>,
}

#[derive(Debug, S2ProtoUnpack, SimpleObject)]
#[s2_grpc(message_type = "flo_grpc::game::Map")]
pub struct Map {
  pub sha1: Vec<u8>,
  pub checksum: u32,
  pub name: String,
  pub path: String,
}

#[derive(Debug, S2ProtoUnpack, SimpleObject)]
#[s2_grpc(message_type = "flo_grpc::node::Node")]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub country_id: String,
}

#[derive(Debug, S2ProtoUnpack, SimpleObject)]
#[s2_grpc(message_type = "flo_grpc::game::Slot")]
pub struct Slot {
  pub player: Option<Player>,
  pub settings: SlotSettings,
}

#[derive(Debug, S2ProtoUnpack, SimpleObject)]
#[s2_grpc(message_type = "flo_grpc::game::SlotSettings")]
pub struct SlotSettings {
  pub team: i32,
  pub color: i32,
  pub computer: i32,
  pub handicap: i32,
  pub status: i32,
  #[s2_grpc(proto_enum)]
  pub race: Race,
}

#[derive(Debug, S2ProtoUnpack, SimpleObject)]
#[s2_grpc(message_type = "flo_grpc::player::PlayerRef")]
pub struct Player {
  pub id: i32,
  pub name: String,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, S2ProtoEnum, Enum)]
#[s2_grpc(proto_enum_type(
  flo_grpc::game::Race,
))]
pub enum Race {
  Human,
  Orc,
  NightElf,
  Undead,
  Random,
}