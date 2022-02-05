use async_graphql::{Context, Object, Result, Schema, SimpleObject, Subscription, Union};
use flo_observer_edge::{
  game::snapshot::GameSnapshot,
  game::{
    event::{GameListUpdateEvent, GameUpdateEvent},
    snapshot::GameSnapshotWithStats,
  },
  FloObserverEdgeHandle,
};
use tokio_stream::{once, Stream, StreamExt};

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
  ) -> Result<ObserverTokenPayload> {
    let handle: &FloObserverEdgeHandle = ctx.data()?;
    let game = handle.get_game(game_id).await?;
    let delay_secs = Some(if game.mask_player_names { 15 * 60 } else { 3 * 60 });
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
