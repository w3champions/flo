use crate::error::Result;
use flo_net::w3gs::W3GSPacketTypeId;
use flo_observer::record::GameRecordData;
use flo_w3gs::action::IncomingAction;
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

  pub fn remaining_millis(&self) -> u64 {
    let mut time = 0;
    for record in &self.records {
      if let GameRecordData::W3GS(pkt) = record {
        match pkt.type_id() {
          W3GSPacketTypeId::IncomingAction | W3GSPacketTypeId::IncomingAction2 => {
            let time_increment_ms =
              IncomingAction::peek_time_increment_ms(pkt.payload.as_ref()).ok().unwrap_or_default();
            time += time_increment_ms as u64;
          },
          _ => {}
        }
      }
    }
    time
  }
}

impl Stream for MemorySource {
  type Item = Result<GameRecordData>;

  fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Poll::Ready(self.records.pop_front().map(Ok))
  }
}
