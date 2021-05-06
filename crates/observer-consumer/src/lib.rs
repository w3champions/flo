mod cache;
mod consumer;
mod env;
pub mod error;
mod fs;

use crate::cache::Cache;
use crate::consumer::StartShardConsumer;
use crate::error::Error;
use consumer::ShardConsumer;
use error::Result;
use flo_observer::{KINESIS_CLIENT, KINESIS_STREAM_NAME};
use flo_state::{Actor, Owner};
use rusoto_kinesis::Kinesis;
use std::collections::BTreeMap;

pub struct FloObserver {
  shards: BTreeMap<String, Owner<ShardConsumer>>,
}

impl FloObserver {
  pub async fn serve() -> Result<()> {
    use rusoto_kinesis::ListShardsInput;

    let mut cache = Cache::connect().await?;

    let shards = KINESIS_CLIENT
      .list_shards(ListShardsInput {
        stream_name: Some(KINESIS_STREAM_NAME.clone()),
        ..Default::default()
      })
      .await?;

    let shard_ids: Vec<_> = shards
      .shards
      .ok_or_else(|| Error::NoShards)?
      .into_iter()
      .map(|shard| shard.shard_id)
      .collect();
    tracing::info!("shards: {:?}", shard_ids);

    let state = Self {
      shards: shard_ids
        .into_iter()
        .map(|id| {
          let actor = ShardConsumer::new(id.clone(), cache.clone()).start();
          (id, actor)
        })
        .collect(),
    };

    let game_ids = cache.list_games().await?;
    let mut shard_games = BTreeMap::new();
    for id in game_ids {
      if let Some(game) = cache.get_game_state(id).await? {
        shard_games
          .entry(game.shard_id.clone())
          .or_insert_with(|| vec![])
          .push(game);
      }
    }

    for (shard_id, actor) in state.shards.iter() {
      let recovered_games = if let Some(recovered_games) = shard_games.remove(shard_id) {
        tracing::info!(
          "recovered shard games: {} = {}",
          shard_id,
          recovered_games.len()
        );
        recovered_games
      } else {
        vec![]
      };
      actor.send(StartShardConsumer { recovered_games }).await??;
    }

    std::future::pending::<()>().await;

    Ok(())
  }
}
