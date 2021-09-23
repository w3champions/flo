//! LRU cache with unlimited filesystem swap space

use crate::error::Result;
use crate::streamer::{GamePartStream, GamePartSender, self};
use bytes::Bytes;
use flo_state::{Actor, Handler, Message, async_trait};
use std::collections::{BTreeMap, BinaryHeap};
use std::path::PathBuf;
use std::time::{Instant, Duration};
use tokio::fs::{self, File};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use uluru::LRUCache;


// const MAX_IN_MEM_GAME: usize = 300;
const SWAP_EVICT_AFTER: Duration = Duration::from_secs(3600);
const GC_INTERVAL: Duration = Duration::from_secs(600);

pub struct MemCacheMgr<const IN_MEMORY_NODES: usize>  {
  mem_store: LRUCache<MemNode, IN_MEMORY_NODES>,
  subs_map: BTreeMap<i32, Vec<GamePartSender>>,
  swap_base_path: PathBuf,
  swap_store: BTreeMap<i32, SwapNode>,
  swap_touched_heap: BinaryHeap<SwapTouchedHeapNode>,
}

pub trait IntoPartsIter {
  type Iter: Iterator<Item = Bytes> + Clone + std::fmt::Debug;
  fn into_parts_iter(self) -> Self::Iter;
}

impl IntoPartsIter for Bytes {
  type Iter = std::iter::Once<Bytes>;
  fn into_parts_iter(self) -> Self::Iter {
    std::iter::once(self)
  }
}

impl IntoPartsIter for Vec<Bytes> {
  type Iter = std::vec::IntoIter<Bytes>;
  fn into_parts_iter(self) -> Self::Iter {
    self.into_iter()
  }
}

impl<const N: usize> MemCacheMgr<N> {
  pub fn new(swap_base_path: PathBuf) -> Self {
    Self {
        mem_store: LRUCache::default(),
        subs_map: BTreeMap::new(),
        swap_base_path,
        swap_store: BTreeMap::new(),
        swap_touched_heap: BinaryHeap::new(),
    }
  }

  pub async fn subscribe(&mut self, game_id: i32) -> Result<Option<GamePartStream>> {
    let stream = if let Some(mem) = self.mem_store.lookup(|v| {
      v.data_of(game_id).map(|v| {
        v.parts.clone()
      })
    }) {
      tracing::debug!(game_id, "subscribing: mem, parts = {}", mem.len());
      let (tx, rx) = streamer::channel(mem);
      self.subs_map.entry(game_id).or_insert_with(|| vec![]).push(tx);
      Some(rx)
    } else {
      if let Some(node) = self.swap_store.remove(&game_id) {
        let bytes = self.move_to_mem(game_id, node.file).await?;
        tracing::debug!(game_id, "subscribing: swap, bytes len = {}", bytes.len());
        let (tx, rx) = streamer::channel(vec![bytes]);
        self.subs_map.entry(game_id).or_insert_with(|| vec![]).push(tx);
        Some(rx)
      } else {
        None
      }
    };
    Ok(stream)
  }

  pub async fn insert<T: IntoPartsIter>(&mut self, game_id: i32, parts: T) -> Result<()> {
    let mem = self.mem_store.find(|v| v.is_data_of(game_id));
    let iter = parts.into_parts_iter();
    let remove_subs_entry = if let Some(senders) = self.subs_map.get_mut(&game_id) {
      let parts = iter.clone().collect::<Vec<_>>();
      senders.retain(move |sender| {
        sender.send_or_drop(parts.clone())
      });
      senders.is_empty()
    } else {
      false
    };
    if remove_subs_entry {
      self.subs_map.remove(&game_id);
    }

    if let Some(MemNode::Data(v)) = mem {
      v.parts.extend(iter);
    } else {
      if let Some(node) = self.swap_store.get_mut(&game_id) {
        for part in iter {
          node.file.write_all(&part).await?;
        }
        let last_touched_at = Instant::now();
        node.last_touched_at = last_touched_at;
        self.swap_touched_heap.push(SwapTouchedHeapNode {
          game_id,
          last_touched_at,
        });
      } else {
        if let Some(MemNode::Data(removed)) = self.mem_store.insert(MemNode::Data(Data {
          game_id,
          parts: iter.collect(),
        })) {
          self.move_to_swap(removed).await?;
        }
      }
    }
    Ok(())
  }

  pub async fn remove(&mut self, game_id: i32) -> Result<()> {
    let mut found = false;
    self.mem_store.lookup(|node| -> Option<()> {
      if node.is_data_of(game_id) {
        found = true;
        *node = MemNode::Vacant;
      }
      None
    });
    if !found {
      if let Some(mut node) = self.swap_store.remove(&game_id) {
        node.file.flush().await?;
      }
      fs::remove_file(self.swap_base_path.join(game_id.to_string())).await?;
    }
    self.subs_map.remove(&game_id);
    Ok(())
  }

  pub fn get_metrics(&self) -> MemCacheMgrMetrics {
    MemCacheMgrMetrics {
      mem_count: self.mem_store.iter().filter(|v| !v.is_vacant()).count(),
      swap_count: self.swap_store.len(),
      subcriptions: self.subs_map.iter().map(|(k, v)| {
        (*k, v.len())
      }).collect(),
    }
  }

  async fn move_to_swap(&mut self, data: Data) -> Result<()> {
    let game_id = data.game_id;
    tracing::debug!(game_id, "move_to_swap: parts = {}", data.parts.len());
    let path = self.swap_base_path.join(data.game_id.to_string());
    let mut file = File::create(&path).await?;
    for part in data.parts {
      file.write_all(&part).await?;
    }
    let last_touched_at = Instant::now();
    self.swap_store.insert(data.game_id, SwapNode { file, last_touched_at });
    self.swap_touched_heap.push(SwapTouchedHeapNode {
      game_id,
      last_touched_at,
    });
    Ok(())
  }

  async fn move_to_mem(&mut self, game_id: i32, mut out_file: File) -> Result<Bytes> {
    tracing::debug!(game_id, "move_to_mem");
    out_file.sync_all().await?;
    out_file.flush().await?;
    drop(out_file);
    let path = self.swap_base_path.join(game_id.to_string());
    let mut buffer = Vec::new();
    let mut file = File::open(path).await?;
    file.read_to_end(&mut buffer).await?;
    let bytes = Bytes::from(buffer);
    let data = Data {
      game_id,
      parts: vec![bytes.clone()]
    };
    if let Some(MemNode::Data(removed)) = self.mem_store.insert(MemNode::Data(data)) {
      self.move_to_swap(removed).await?;
    }
    Ok(bytes)
  }

  async fn evict_swap(&mut self, game_id: i32) -> Result<()> {
    if let Some(mut node) = self.swap_store.remove(&game_id) {
      node.file.flush().await?;
    }
    let path = self.swap_base_path.join(game_id.to_string());
    fs::remove_file(path).await?;
    Ok(())
  }

  async fn gc(&mut self) -> Result<()> {
    let now = Instant::now();
    let mut evicted = 0;
    while let Some(node) = self.swap_touched_heap.pop() {
      if self.swap_store.get(&node.game_id).map(|v| v.last_touched_at == node.last_touched_at).unwrap_or_default() {
        if now - node.last_touched_at > SWAP_EVICT_AFTER {
          match self.evict_swap(node.game_id).await {
            Ok(_) => {
              evicted += 1;
            },
            Err(err) => {
              self.swap_touched_heap.push(node);
              return Err(err)
            }
          }
        } else {
          break;
        }
      }
    }
    tracing::debug!("gc: evicted = {}", evicted);
    Ok(())
  }
}

enum MemNode {
  Vacant,
  Data(Data),
}

impl MemNode {
  fn is_vacant(&self) -> bool {
    match *self {
      MemNode::Vacant => true,
      MemNode::Data(_) => false,
    }
  }

  fn is_data_of(&self, id: i32) -> bool {
    match *self {
      MemNode::Vacant => false,
      MemNode::Data(ref data) => data.game_id == id,
    }
  }

  fn data_of(&mut self, id: i32) -> Option<&mut Data> {
    match *self {
      MemNode::Vacant => None,
      MemNode::Data(ref mut data) => if data.game_id == id {
        Some(data)
      } else {
        None
      },
    }
  }
}

struct Data {
  game_id: i32,
  parts: Vec<Bytes>,
}

struct SwapNode {
  file: File,
  last_touched_at: Instant,
}

struct SwapTouchedHeapNode {
  game_id: i32,
  last_touched_at: Instant,
}

impl PartialEq for SwapTouchedHeapNode {
  fn eq(&self, other: &Self) -> bool {
    self.last_touched_at == other.last_touched_at
  }
}

impl Eq for SwapTouchedHeapNode {}

impl PartialOrd for SwapTouchedHeapNode {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    self.cmp(other).into()
  }
}

impl Ord for SwapTouchedHeapNode {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    self.last_touched_at.cmp(&other.last_touched_at).reverse()
  }
}

#[async_trait]
impl<const N: usize> Actor for MemCacheMgr<N> {
  async fn started(&mut self, ctx: &mut flo_state::Context<Self>) {
    use tokio::time::sleep;
    let addr = ctx.addr();
    ctx.spawn(async move {
      loop {
        if let Err(err) = addr.send(Gc).await {
          tracing::error!("gc: {}", err);
        }
        sleep(GC_INTERVAL).await;
      }
    });
  }
}

struct Gc;
impl Message for Gc {
  type Result = Result<()>;
}

#[async_trait]
impl<const N: usize> Handler<Gc> for MemCacheMgr<N> {
  async fn handle(&mut self, _ctx: &mut flo_state::Context<Self>, _: Gc) -> Result<()> {
    self.gc().await
  }
}

#[derive(Debug)]
pub struct MemCacheMgrMetrics {
  pub mem_count: usize,
  pub swap_count: usize,
  pub subcriptions: BTreeMap<i32, usize>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;
  use once_cell::sync::Lazy;
  use tempfile::TempDir;
  static TEMP_DIR: Lazy<TempDir> = Lazy::new(|| {
    let dir = tempfile::tempdir().unwrap();
    dir
  });
  static SWAP_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let path = TEMP_DIR.path().join("lru");
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
  });

  #[tokio::test]
  async fn test_mem_cache() {
    use tokio::sync::mpsc;
    use futures::stream::StreamExt;

    tracing_subscriber::fmt::init();

    let mut tasks = vec![];
    let (tx, mut rx) = mpsc::channel::<(i32, Vec<Bytes>)>(10);

    let mut mgr = MemCacheMgr::<1>::new(SWAP_DIR.clone());
    let initial = Bytes::copy_from_slice(&[0]);
    let parts: Vec<_> = (1..10_i32).map(|v| {
      let bytes = v.to_be_bytes();
      Bytes::copy_from_slice(bytes.as_ref())
    }).collect();

    fn flatten(parts: Vec<Bytes>) -> Bytes {
      let mut b = vec![];
      parts.into_iter().fold(&mut b, |init, item| {
        init.extend_from_slice(item.as_ref());
        init
      });
      Bytes::from(b)
    }

    let expected = {
      let mut v = parts.clone();
      v.insert(0, initial.clone());
      flatten(v)
    };

    mgr.insert(1, initial.clone()).await.unwrap();

    let m = mgr.get_metrics();
    assert_eq!(m.mem_count, 1);
    assert_eq!(m.swap_count, 0);

    let make_collector = |id, s: GamePartStream| {
      let tx = tx.clone();
      async move {
        let values = s.collect::<Vec<_>>().await;
        tx.send((id, values)).await.unwrap();
      }
    };

    {
      let s = mgr.subscribe(1).await.unwrap().unwrap();
      let task = tokio::spawn(make_collector(1, s));
      tasks.push(task);
    }

    for i in 1..5 {
      mgr.insert(1, parts[i - 1].clone()).await.unwrap();
    }

    mgr.insert(2, initial.clone()).await.unwrap();

    {
      let m = mgr.get_metrics();
      assert_eq!(m.mem_count, 1);
      assert_eq!(m.swap_count, 1);
    }


    
    {
      let s = mgr.subscribe(1).await.unwrap().unwrap();
      let task = tokio::spawn(make_collector(1, s));
      tasks.push(task);
    }

    for i in 5..8 {
      mgr.insert(1, parts[i - 1].clone()).await.unwrap();
    }

    for i in 8..10 {
      mgr.insert(1, parts[i - 1].clone()).await.unwrap();
    }

    for i in 1..10 {
      mgr.insert(2, parts[i - 1].clone()).await.unwrap();
    }

    {
      let s = mgr.subscribe(2).await.unwrap().unwrap();
      let task = tokio::spawn(make_collector(2, s));
      tasks.push(task);
    }

    {
      let task = tokio::spawn(async move {
        let mut map = BTreeMap::new();
        while let Some((id, parts)) = rx.recv().await {
          map.entry(id).or_insert_with(|| vec![]).push(parts);
        }
        assert_eq!(map.keys().cloned().collect::<Vec<_>>(), vec![1, 2]);
        for (_, results) in map {
          for r in results {
            assert_eq!(flatten(r), expected);
          }
        }
      });
      tasks.push(task);
    }

    mgr.remove(1).await.unwrap();
    mgr.remove(2).await.unwrap();

    {
      let m = mgr.get_metrics();
      assert_eq!(m.mem_count, 0);
      assert_eq!(m.swap_count, 0);
      assert!(m.subcriptions.is_empty());
    }

    drop(tx);

    futures::future::join_all(tasks).await.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
  }
}
