use backoff::ExponentialBackoff;
use backoff::backoff::Backoff;
use chrono::{DateTime, Utc};
use flo_observer::record::{GameRecordData, RTTStats};
use flo_state::{Addr, Actor, async_trait, Message, Context, Handler};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use tracing::{Span, Instrument};
use crate::error::{Result, Error};
use crate::services::Services;

pub struct GameHandler {
  services: Services,
  meta: GameMeta,
  game: FetchGameState,
  records: Vec<GameRecordData>,
  span: Span,
}

impl GameHandler {
  pub fn new(services: Services, meta: GameMeta, records: Vec<GameRecordData>) -> Self {
    let game_id = meta.id;
    Self {
      services,
      meta,
      game: FetchGameState::Loading,
      records,
      span: tracing::info_span!("game_handler", game_id),
    }
  }

  async fn handle_records(&mut self, records: Vec<GameRecordData>) {
    self.records.extend(records);
  }
}

#[async_trait]
impl Actor for GameHandler {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let addr = ctx.addr();
    let services = self.services.clone();
    let game_id = self.meta.id;
    // fetch game in background
    ctx.spawn(async move {
      let mut backoff = ExponentialBackoff::default();
      loop {
        match services.controller.fetch_game(game_id).await {
          Ok(game) => {
            addr.notify(FetchGameResult(Ok(game))).await.ok();
            break;
          },
          Err(err @ Error::InvalidGameId(_)) => {
            addr.notify(FetchGameResult(Err(err))).await.ok();
            break;
          },
          Err(err) => {
            if let Some(d) = backoff.next_backoff() {
              tracing::error!("fetch game: {}, retry in {:?}...", err, d);
              tokio::time::sleep(d).await;
            } else {
              addr.notify(FetchGameResult(Err(err))).await.ok();
              break;
            }
          }
        }
      }
    }.instrument(self.span.clone()));
  }
}

pub struct HandleRecords(pub Vec<GameRecordData>);

impl Message for HandleRecords {
  type Result = ();
}

#[async_trait]
impl Handler<HandleRecords> for GameHandler {
  async fn handle(&mut self, _: &mut Context<Self> , HandleRecords(records): HandleRecords) {
    self.handle_records(records).await;
  }
}

struct FetchGameResult(Result<Game>);

impl Message for FetchGameResult {
  type Result = ();
}

#[async_trait]
impl Handler<FetchGameResult> for GameHandler {
  async fn handle(&mut self, _: &mut Context<Self> , FetchGameResult(r): FetchGameResult) {
    match r {
      Ok(game) => {
        self.game = FetchGameState::Loaded(game);
      },
      Err(err) => {
        self.game = FetchGameState::Failed(err);
      },
    }
  }
}

pub struct GameMeta {
  pub id: i32,
  pub started_at: DateTime<Utc>,
}

enum FetchGameState {
  Loading,
  Loaded(Game),
  Failed(Error),
}

impl FetchGameState {
  fn get(&self) -> Result<&Game> {
    match self {
      FetchGameState::Loading => Err(Error::GameNotReady("still loading".to_string())),
      FetchGameState::Loaded(ref game) => Ok(game),
      FetchGameState::Failed(ref e) => Err(Error::GameNotReady(e.to_string())),
    }
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::Game")]
pub struct Game {
  pub id: i32,
  pub name: String,
  pub slots: Vec<Slot>,
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::Slot")]
pub struct Slot {
  pub player: Option<Player>,
  pub settings: SlotSettings,
}

#[derive(Debug, S2ProtoUnpack)]
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

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::player::PlayerRef")]
pub struct Player {
  pub id: i32,
  pub name: String,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, S2ProtoEnum)]
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