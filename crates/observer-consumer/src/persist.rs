use crate::error::Result;
use once_cell::sync::Lazy;
use redis::aio::ConnectionManager;
use redis::{pipe, Client};
use std::convert::TryInto;
use std::fmt::{self, Debug, Formatter};
use std::time::SystemTime;

static CLIENT: Lazy<Client> =
  Lazy::new(|| Client::open(&*crate::env::ENV.redis_url).expect("redis client open"));

#[derive(Clone)]
pub struct Persist {
  conn: ConnectionManager,
}

impl Persist {
  pub async fn connect() -> Result<Self> {
    let conn = CLIENT.get_tokio_connection_manager().await?;
    Ok(Self { conn })
  }

  const SHARD_HASH_PREFIX: &'static str = "flo_observer:shard";
  const SHARD_HASH_FINISHED_SEQ_NUMBER: &'static str = "finished_seq_number";

  pub async fn get_shard_finished_seq(&mut self, shard_id: &str) -> Result<Option<String>> {
    let key = format!("{}:{}", Self::SHARD_HASH_PREFIX, shard_id);
    let res: Option<String> = redis::cmd("HGET")
      .arg(key)
      .arg(Self::SHARD_HASH_FINISHED_SEQ_NUMBER)
      .query_async(&mut self.conn)
      .await?;

    Ok(res)
  }

  pub async fn set_shard_finished_seq(&mut self, shard_id: &str, value: &str) -> Result<()> {
    let key = format!("{}:{}", Self::SHARD_HASH_PREFIX, shard_id);
    redis::cmd("HSET")
      .arg(key)
      .arg(Self::SHARD_HASH_FINISHED_SEQ_NUMBER)
      .arg(value)
      .query_async::<_, ()>(&mut self.conn)
      .await?;
    Ok(())
  }

  const GAME_SET_KEY: &'static str = "flo_observer:games";
  const GAME_HASH_PREFIX: &'static str = "flo_observer:game";
  const GAME_HASH_SHARD_ID: &'static str = "shard_id";
  const GAME_HASH_LAST_TOUCH_TIMESTAMP: &'static str = "last_touch_timestamp";

  pub async fn add_game(&mut self, game_id: i32, shard_id: &str) -> Result<()> {
    let game_hash_key = format!("{}:{}", Self::GAME_HASH_PREFIX, game_id);
    pipe()
      .atomic()
      .cmd("SADD")
      .arg(Self::GAME_SET_KEY)
      .arg(&game_id.to_le_bytes() as &[u8])
      .cmd("HSET")
      .arg(&game_hash_key)
      .arg(Self::GAME_HASH_SHARD_ID)
      .arg(shard_id)
      .query_async::<_, ()>(&mut self.conn)
      .await?;
    Ok(())
  }

  pub async fn remove_game(&mut self, game_id: i32) -> Result<()> {
    redis::cmd("SREM")
      .arg(Self::GAME_SET_KEY)
      .arg(&game_id.to_le_bytes() as &[u8])
      .query_async::<_, ()>(&mut self.conn)
      .await?;

    let game_hash_key = format!("{}:{}", Self::GAME_HASH_PREFIX, game_id);
    pipe()
      .atomic()
      .cmd("SREM")
      .arg(Self::GAME_SET_KEY)
      .arg(&game_id.to_le_bytes() as &[u8])
      .cmd("DEL")
      .arg(&game_hash_key)
      .query_async::<_, ()>(&mut self.conn)
      .await?;

    Ok(())
  }

  pub async fn list_games(&mut self) -> Result<Vec<i32>> {
    let list: Vec<Vec<u8>> = redis::cmd("SMEMBERS")
      .arg(Self::GAME_SET_KEY)
      .query_async(&mut self.conn)
      .await?;
    let mut list = list
      .into_iter()
      .filter_map(|bytes| {
        if bytes.len() == 4 {
          Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
          None
        }
      })
      .collect::<Vec<_>>();
    list.sort();
    Ok(list)
  }

  pub async fn get_game_state(&mut self, game_id: i32) -> Result<Option<PersistGameState>> {
    let key = format!("{}:{}", Self::GAME_HASH_PREFIX, game_id);
    let (shard_id, last_touch_timestamp): (Option<String>, Option<Vec<u8>>) = redis::cmd("HMGET")
      .arg(key)
      .arg(Self::GAME_HASH_SHARD_ID)
      .arg(Self::GAME_HASH_LAST_TOUCH_TIMESTAMP)
      .query_async(&mut self.conn)
      .await?;

    let last_touch_timestamp = last_touch_timestamp.and_then(|v| {
      if v.len() == 8 {
        return Some(u64::from_le_bytes(v.try_into().unwrap()));
      }
      None
    });

    Ok(shard_id.map(|shard_id| PersistGameState {
      id: game_id,
      shard_id,
      last_touch_timestamp,
    }))
  }

  #[allow(unused)]
  pub async fn touch_game(&mut self, game_id: i32) -> Result<()> {
    let key = format!("{}:{}", Self::GAME_HASH_PREFIX, game_id);
    redis::cmd("HSET")
      .arg(&key)
      .arg(Self::GAME_HASH_LAST_TOUCH_TIMESTAMP)
      .arg(&Self::timestamp().to_le_bytes() as &[u8])
      .query_async::<_, ()>(&mut self.conn)
      .await?;
    Ok(())
  }

  fn timestamp() -> u64 {
    SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_millis() as u64
  }
}

impl Debug for Persist {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("Cache").finish()
  }
}

#[derive(Debug)]
pub struct PersistGameState {
  pub id: i32,
  pub shard_id: String,
  pub last_touch_timestamp: Option<u64>,
}

#[tokio::test]
async fn test_shared_finished_seq() {
  dotenv::dotenv().unwrap();

  let mut c = Persist::connect().await.unwrap();

  redis::cmd("HDEL")
    .arg(format!("{}:{}", Persist::SHARD_HASH_PREFIX, "FAKE"))
    .arg(Persist::SHARD_HASH_FINISHED_SEQ_NUMBER)
    .query_async::<_, ()>(&mut c.conn)
    .await
    .unwrap();

  let v = c.get_shard_finished_seq("FAKE").await.unwrap();
  assert_eq!(v, None);

  c.set_shard_finished_seq("FAKE", "123").await.unwrap();
  let v = c.get_shard_finished_seq("FAKE").await.unwrap();
  assert_eq!(v, Some("123".to_string()))
}

#[tokio::test]
async fn test_game_set() {
  dotenv::dotenv().unwrap();

  let mut c = Persist::connect().await.unwrap();

  redis::cmd("DEL")
    .arg(Persist::GAME_SET_KEY)
    .query_async::<_, ()>(&mut c.conn)
    .await
    .unwrap();

  let list = c.list_games().await.unwrap();
  assert!(list.is_empty());

  c.add_game(0x11, "a").await.unwrap();
  c.add_game(0x22, "a").await.unwrap();
  c.add_game(0x33, "a").await.unwrap();

  let list = c.list_games().await.unwrap();
  assert_eq!(list, vec![0x11, 0x22, 0x33]);

  c.remove_game(0x22).await.unwrap();

  let list = c.list_games().await.unwrap();
  assert_eq!(list, vec![0x11, 0x33]);
}

#[tokio::test]
async fn test_game_state() {
  dotenv::dotenv().unwrap();
  let game_id = i32::MAX;

  let mut c = Persist::connect().await.unwrap();

  c.remove_game(game_id).await.unwrap();

  let v = c.get_game_state(game_id).await.unwrap();
  assert!(v.is_none());

  c.add_game(game_id, "shard").await.unwrap();
  let v = c.get_game_state(game_id).await.unwrap().unwrap();
  assert_eq!(v.shard_id, "shard");
  assert_eq!(v.last_touch_timestamp, None);

  c.touch_game(game_id).await.unwrap();

  let v = c.get_game_state(game_id).await.unwrap().unwrap();
  assert!(v.last_touch_timestamp.is_some());
}
