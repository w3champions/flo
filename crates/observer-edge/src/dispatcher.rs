use lru::LruCache;
use flo_state::{Owner, Actor, async_trait, Context, Addr};
use flo_kinesis::data_stream::DataStreamIterator;
use flo_kinesis::iterator::{Chunk};
use crate::error::{Result, Error};
use crate::game::GameHandler;
use tokio_stream::StreamExt;

const MAX_GAMES: usize = 100;

pub struct Dispatcher {
  games: LruCache<i32, Owner<GameHandler>>,
  iter: Option<DataStreamIterator>,
}

impl Dispatcher {
  pub fn new(iter: DataStreamIterator) -> Self {
    Self {
      games: LruCache::new(MAX_GAMES),
      iter: Some(iter),
    }
  }

  // async fn run_loop(addr: Addr<Self>, iter: Option<DataStreamIterator>) {
  //   let mut iter = if let Some(v) = iter {
  //     v
  //   } else {
  //     return;
  //   };

  //   while let Some(v) = iter.next().await {
  //     addr.notify(message).await;
  //   }
  // }

  async fn handle_chunk(&mut self, chunk: Chunk) {
    
  }
}
