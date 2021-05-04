use redis::{Client, Commands};
use redis::aio::ConnectionManager;
use crate::error::Result;
use once_cell::sync::Lazy;
use std::fmt::{self, Debug, Formatter};

static CLIENT: Lazy<Client> = Lazy::new(|| {
  Client::open(&*crate::env::ENV.redis_url).expect("redis client open")
});

#[derive(Clone)]
pub struct Cache {
  conn: ConnectionManager,
}

impl Cache {
  pub async fn connect() -> Result<Self> {
    let conn = CLIENT.get_tokio_connection_manager().await?;
    Ok(Self {
      conn,
    })
  }

  const SHARD_HASH_PREFIX: &'static str = "flo_observer:shard";
  const KEY_FINISHED_SEQ_NUMBER: &'static str = "finished_seq_number";

  pub async fn get_shard_finished_seq(&mut self, shard_id: &str) -> Result<Option<String>> {
    let key = format!("{}:{}", Self::SHARD_HASH_PREFIX, shard_id);
    let res: Option<String> = redis::cmd("HGET").arg(key).arg(Self::KEY_FINISHED_SEQ_NUMBER)
      .query_async(&mut self.conn)
      .await?;

    Ok(res)
  }

  pub async fn set_shard_finished_seq(&mut self, shard_id: &str, value: &str) -> Result<()> {
    let key = format!("{}:{}", Self::SHARD_HASH_PREFIX, shard_id);
    redis::cmd("HSET").arg(key).arg(Self::KEY_FINISHED_SEQ_NUMBER).arg(value)
      .query_async::<_, ()>(&mut self.conn)
      .await?;
    Ok(())
  }
}

impl Debug for Cache {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("Persist")
      .finish()
  }
}

#[tokio::test]
async fn test_shared_finished_seq() {
  dotenv::dotenv().unwrap();

  let mut c = Cache::connect().await.unwrap();

  redis::cmd("HDEL")
    .arg(format!("{}:{}", Cache::SHARD_HASH_PREFIX, "FAKE"))
    .arg(Cache::KEY_FINISHED_SEQ_NUMBER)
    .query_async::<_, ()>(&mut c.conn).await.unwrap();

  let v = c.get_shard_finished_seq("FAKE").await.unwrap();
  assert_eq!(v, None);

  c.set_shard_finished_seq("FAKE", "123").await.unwrap();
  let v = c.get_shard_finished_seq("FAKE").await.unwrap();
  assert_eq!(v, Some("123".to_string()))
}