use futures::stream::Stream;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{interval_at, Instant, Interval};

use flo_w3gs::protocol::action::{IncomingAction, PlayerAction, TimeSlot};
use futures::task::{Context, Poll};

#[derive(Debug)]
pub struct ActionTickStream {
  paused: bool,
  step: u32,
  step_duration: Duration,
  interval: Option<Interval>,
  tick: Tick,
  time: u32,
  last_instant: Instant,
}

impl ActionTickStream {
  pub const MIN_STEP: u32 = 15;

  pub fn new(step: u32) -> Self {
    let step = std::cmp::max(Self::MIN_STEP, step);
    ActionTickStream {
      step,
      step_duration: Duration::from_millis(step as u64),
      paused: false,
      interval: None,
      tick: Tick {
        time_increment_ms: step,
        actions: vec![],
      },
      time: 0,
      last_instant: Instant::now(),
    }
  }

  pub fn set_step(&mut self, value: u32) {
    self.step = std::cmp::max(Self::MIN_STEP, value);
    self.step_duration = Duration::from_millis(value as u64);
    self.interval.take();
  }

  pub fn pause(&mut self) {
    self.paused = true;
    self.interval.take();
  }

  pub fn resume(&mut self) {
    if !self.paused {
      return;
    }
    self.paused = false;
  }

  pub fn time(&self) -> u32 {
    self.time
  }

  pub fn add_player_action(&mut self, action: PlayerAction) {
    self.tick.actions.push(action)
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
    if self.paused {
      return Poll::Pending;
    }

    let instant = match self.interval {
      Some(ref mut interval) => futures::ready!(Pin::new(interval).poll_next(cx)),
      None => {
        let mut interval = interval_at(Instant::now() + self.step_duration, self.step_duration);
        match Pin::new(&mut interval).poll_next(cx) {
          Poll::Ready(_) => unreachable!(),
          Poll::Pending => {
            self.last_instant = Instant::now();
            self.interval = Some(interval);
            return Poll::Pending;
          }
        }
      }
    }
    .expect("interval stream never ends");

    let d = instant - self.last_instant;
    self.last_instant = instant;

    let step = self.step;
    let tick = {
      std::mem::replace(
        &mut self.tick,
        Tick {
          time_increment_ms: d.as_millis() as u32,
          actions: vec![],
        },
      )
    };
    Poll::Ready(Some(tick))
  }
}
