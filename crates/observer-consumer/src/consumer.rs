use crate::cache::Cache;
use flo_state::{Actor, Owner};
use rusoto_kinesis::Kinesis;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ShardConsumer {
  shard_id: String,
  cache: Cache,
}

impl ShardConsumer {
  pub fn new(shard_id: String, cache: Cache) -> Self {
    Self { shard_id, cache }
  }
}

#[tokio::test]
async fn test_list_shards() {
  use flo_observer::KINESIS_CLIENT;
  use rusoto_kinesis::{Kinesis, ListShardsInput};

  dotenv::dotenv().unwrap();

  let shards = KINESIS_CLIENT
    .list_shards(ListShardsInput {
      stream_name: Some(flo_observer::KINESIS_STREAM_NAME.clone()),
      ..Default::default()
    })
    .await
    .unwrap();

  dbg!(shards);
}
