use futures::stream::Stream;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{delay_for, Delay};

use flo_w3gs::protocol::action::{IncomingAction, OutgoingAction, PlayerAction, TimeSlot};
use futures::task::{Context, Poll};

#[derive(Debug)]
pub struct ActionTickStream {
  paused: bool,
  step: u32,
  delay: Option<Delay>,
  tick: Tick,
}

impl ActionTickStream {
  pub const MIN_STEP: u32 = 15;

  pub fn new(step: u32) -> Self {
    let step = std::cmp::max(Self::MIN_STEP, step);
    ActionTickStream {
      step,
      paused: false,
      delay: None,
      tick: Tick {
        time_increment_ms: step,
        actions: vec![],
      },
    }
  }

  pub fn set_step(&mut self, value: u32) {
    self.step = std::cmp::max(Self::MIN_STEP, value);
  }

  pub fn pause(&mut self) {
    self.paused = true;
    self.delay.take();
  }

  pub fn resume(&mut self) {
    if !self.paused {
      return;
    }
    self.paused = false;
  }

  pub fn add_player_action(&mut self, slot_player_id: u8, action: OutgoingAction) {
    self.tick.actions.push(PlayerAction {
      player_id: slot_player_id,
      data: action.data,
    })
  }

  fn register_next(&mut self, cx: &mut Context<'_>) {
    let mut delay = delay_for(Duration::from_millis(self.step as u64));
    match Pin::new(&mut delay).poll(cx) {
      Poll::Ready(_) => unreachable!(),
      Poll::Pending => {
        self.delay.replace(delay);
      }
    }
  }
}

#[derive(Debug)]
pub struct Tick {
  pub time_increment_ms: u32,
  pub actions: Vec<PlayerAction>,
}

impl From<Tick> for IncomingAction {
  fn from(tick: Tick) -> Self {
    IncomingAction(TimeSlot {
      time_increment_ms: tick.time_increment_ms as u16,
      actions: tick.actions,
    })
  }
}

impl Stream for ActionTickStream {
  type Item = Tick;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    match self.delay {
      Some(ref mut delay) => futures::ready!(Pin::new(delay).poll(cx)),
      None => {
        self.register_next(cx);
        return Poll::Pending;
      }
    }

    let step = self.step;
    let tick = std::mem::replace(
      &mut self.tick,
      Tick {
        time_increment_ms: step,
        actions: vec![],
      },
    );
    self.register_next(cx);
    Poll::Ready(Some(tick))
  }
}
