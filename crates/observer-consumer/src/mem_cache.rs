use crate::error::Result;
use bytes::Bytes;
use flo_state::{async_trait, Actor};
use std::collections::BTreeMap;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use uluru::LRUCache;

const MAX_IN_MEM_GAME: usize = 100;

pub struct MemCacheMgr {
  mem_store: LRUCache<Data, MAX_IN_MEM_GAME>,
  swap_store: BTreeMap<i32, File>,
}

impl MemCacheMgr {
  pub async fn insert(&mut self, game_id: i32, part: Bytes) -> Result<()> {
    let mem = self.mem_store.find(|v| v.game_id == game_id);
    if let Some(v) = mem {
      v.parts.push(part);
    } else {
      if let Some(file) = self.swap_store.get_mut(&game_id) {
        file.write_all(&part).await?;
      } else {
        if let Some(removed) = self.mem_store.insert(Data {
          game_id,
          parts: vec![part],
        }) {
          self.move_to_swap(removed).await?;
        }
      }
    }
    Ok(())
  }

  async fn move_to_swap(&mut self, data: Data) -> Result<()> {
    todo!()
  }
}

struct Data {
  game_id: i32,
  parts: Vec<Bytes>,
}

pub struct MemCacheHandle;

#[async_trait]
impl Actor for MemCacheMgr {}
