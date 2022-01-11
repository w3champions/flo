use crate::error::{Error, Result};
use crate::iterator::{ShardIterator, ShardIteratorConfig};
use flo_observer::record::ObserverRecordSource;
use rusoto_core::{credential::StaticProvider, request::HttpClient};
use rusoto_kinesis::{Kinesis, KinesisClient, ListShardsInput};
use std::env;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio_stream::{StreamMap, Stream};

pub use crate::iterator::{ShardIteratorType, Chunk};

pub struct DataStream {
  client: Arc<KinesisClient>,
  stream_name: String,
  source: ObserverRecordSource,
}

impl DataStream {
  pub fn from_env() -> Self {
    let provider = StaticProvider::new(
      env::var("AWS_ACCESS_KEY_ID").unwrap(),
      env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
      None,
      None,
    );
    let client = HttpClient::new().unwrap();
    let region = env::var("AWS_KINESIS_REGION").unwrap().parse().unwrap();
    Self {
      client: Arc::new(KinesisClient::new_with(client, provider, region)),
      stream_name: env::var("AWS_KINESIS_STREAM_NAME").unwrap(),
      source: ObserverRecordSource::from_str(&env::var("OBSERVER_CONSUMER_SOURCE").unwrap())
        .unwrap(),
    }
  }

  pub async fn into_iter(self, iter_type: ShardIteratorType) -> Result<DataStreamIterator> {
    let shards = self
      .client
      .list_shards(ListShardsInput {
        stream_name: Some(self.stream_name.clone()),
        ..Default::default()
      })
      .await?;
    let shard_ids: Vec<_> = shards
      .shards
      .ok_or_else(|| Error::NoShards)?
      .into_iter()
      .map(|shard| shard.shard_id)
      .collect();

    tracing::debug!("shards: {:?}", shard_ids);

    let shards = shard_ids.into_iter()
      .enumerate()
      .map(|(idx, shard_id)| {
        (idx, ShardIterator::new(ShardIteratorConfig {
          iter_type: iter_type.clone(),
          source: self.source,
          client: self.client.clone(),
          stream_name: self.stream_name.clone(),
          shard_id,
        }))
      }).collect();

    Ok(DataStreamIterator {
      shards
    })
  }
}

pub struct DataStreamIterator {
  shards: StreamMap<usize, ShardIterator>,
}

impl Stream for DataStreamIterator {
  type Item = Chunk;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Pin::new(&mut self.shards).poll_next(cx).map(|item| {
      item.map(|(_, item)| {
        item
      })
    })
  }
}

#[tokio::test]
async fn test_data_stream() {
  use tokio_stream::StreamExt;
  use std::time::Duration;

  dotenv::dotenv().unwrap();
  flo_log_subscriber::init_env_override("flo_kinesis");

  let mut s = DataStream::from_env().into_iter(
    ShardIteratorType::at_timestamp_backward(
      Duration::from_secs(1800)
    )
  ).await.unwrap();
  while let Some(chunk) = s.next().await {
    tracing::info!("chunk: len = {}, millis_behind_latest = {:?}", 
      chunk.game_records.len(),
      chunk.millis_behind_latest
    );
  }
}