use crate::error::Result;
use flo_observer::record::GameRecordData;
use flo_observer_fs::GameDataArchiveReader;
use futures::{Stream, StreamExt};
use std::{
  path::Path,
  pin::Pin,
  task::{Context, Poll},
};

use super::memory::MemorySource;

pub struct ArchiveFileSource {
  inner: MemorySource,
}

impl ArchiveFileSource {
  pub async fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
    Ok(Self {
      inner: MemorySource::new(GameDataArchiveReader::open(path)
        .await?
        .records()
        .collect_vec()
        .await
      ?)
    })
  }
}

impl Stream for ArchiveFileSource {
  type Item = Result<GameRecordData>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.inner.poll_next_unpin(cx)
  }
}
