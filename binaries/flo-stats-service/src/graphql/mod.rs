use async_graphql::{Context, Error, Object, Result, Schema, SimpleObject, Subscription, Union};
use flo_observer_edge::{
  game::snapshot::GameSnapshot,
  game::{
    event::{GameListUpdateEvent, GameUpdateEvent},
    snapshot::GameSnapshotWithStats,
  },
  FloObserverEdgeHandle,
};
use tokio_stream::{once, Stream, StreamExt};

use crate::RequestData;

pub type FloLiveSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;
pub struct QueryRoot;

#[Object]
impl QueryRoot {
  async fn games(&self, ctx: &Context<'_>) -> Result<Vec<GameSnapshot>> {
    let handle: &FloObserverEdgeHandle = ctx.data()?;
    handle.list_games().await.map_err(Into::into)
  }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
  async fn create_observer_token(
    &self,
    ctx: &Context<'_>,
    game_id: i32,
    delay_secs: Option<u16>,
  ) -> Result<ObserverTokenPayload> {
    let handle: &FloObserverEdgeHandle = ctx.data()?;
    let game = handle.get_game(game_id).await?;
    let data: &RequestData = ctx.data()?;

    let delay_secs = if let Some(value) = delay_secs {
      if data.is_admin {
        value as i64
      } else {
        return Err(Error::new("Only admin can specify delay value."));
      }
    } else {
      180
    };
    let delay_secs = if delay_secs == 0 {
      None
    } else {
      Some(delay_secs)
    };
    Ok(ObserverTokenPayload {
      game,
      delay_secs: delay_secs.clone(),
      token: flo_observer::token::create_observer_token(game_id, delay_secs)?,
    })
  }
}

#[derive(SimpleObject)]
pub struct ObserverTokenPayload {
  pub game: GameSnapshot,
  pub delay_secs: Option<i64>,
  pub token: String,
}

pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
  async fn game_list_update_events(
    &self,
    ctx: &Context<'_>,
  ) -> Result<impl Stream<Item = GameListUpdateEventItem>> {
    let handle: &FloObserverEdgeHandle = ctx.data()?;
    let (snapshots, rx) = handle.subscribe_game_list_updates().await?;
    let events = rx
      .into_stream()
      .map(|event| GameListUpdateEventItem::Event(GameListUpdateEventItemEvent { event }));
    Ok(
      once(GameListUpdateEventItem::Initial(
        GameListUpdateEventItemInitial { snapshots },
      ))
      .chain(events),
    )
  }

  async fn game_update_events(
    &self,
    ctx: &Context<'_>,
    id: i32,
  ) -> Result<impl Stream<Item = GameUpdateEventItem>> {
    let handle: &FloObserverEdgeHandle = ctx.data()?;
    let (snapshot, rx) = handle.subscribe_game_updates(id).await?;
    let events = rx.into_stream().map(GameUpdateEventItem::Event);
    Ok(once(GameUpdateEventItem::Initial(snapshot)).chain(events))
  }
}

#[derive(Union)]
pub enum GameListUpdateEventItem {
  Initial(GameListUpdateEventItemInitial),
  Event(GameListUpdateEventItemEvent),
}

#[derive(SimpleObject)]
pub struct GameListUpdateEventItemInitial {
  pub snapshots: Vec<GameSnapshot>,
}

#[derive(SimpleObject)]
pub struct GameListUpdateEventItemEvent {
  pub event: GameListUpdateEvent,
}

#[derive(Union)]
pub enum GameUpdateEventItem {
  Initial(GameSnapshotWithStats),
  Event(GameUpdateEvent),
}
