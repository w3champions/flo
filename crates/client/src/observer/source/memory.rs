use crate::error::Result;
use flo_observer::record::GameRecordData;
use futures::Stream;
use std::{
  collections::VecDeque,
  pin::Pin,
  task::{Context, Poll},
};

pub struct MemorySource {
  records: VecDeque<GameRecordData>,
}

impl MemorySource {
  pub fn new(i: impl IntoIterator<Item = GameRecordData>) -> Self {
    Self {
      records: i
        .into_iter()
        .collect(),
    }
  }
}

impl Stream for MemorySource {
  type Item = Result<GameRecordData>;

  fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Poll::Ready(self.records.pop_front().map(Ok))
  }
}
