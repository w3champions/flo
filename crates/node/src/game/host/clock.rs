use futures::stream::Stream;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio::time::{delay_for, Delay};

use flo_w3gs::protocol::action::PlayerAction;
use futures::task::{Context, Poll};

#[derive(Debug)]
pub struct ActionTickStream {
  step: u16,
  step_duration: Duration,
  delay: Delay,
  actions: Vec<PlayerAction>,
  last_instant: Instant,
}

impl ActionTickStream {
  pub const MIN_STEP: u16 = 15;

  pub fn new(step: u16) -> Self {
    let step = std::cmp::max(Self::MIN_STEP, step);
    let step_duration = Duration::from_millis(step as u64);
    ActionTickStream {
      step,
      step_duration,
      delay: delay_for(step_duration),
      actions: vec![],
      last_instant: Instant::now(),
    }
  }

  pub fn set_step(&mut self, value: u16) {
    self.step = std::cmp::max(Self::MIN_STEP, value);
    self.step_duration = Duration::from_millis(value as u64);
    self.delay = delay_for(self.step_duration);
  }

  pub fn step(&self) -> u16 {
    self.step
  }

  pub fn add_player_action(&mut self, action: PlayerAction) {
    self.actions.push(action)
  }
}

#[derive(Debug)]
pub struct Tick {
  pub time_increment_ms: u16,
  pub actions: Vec<PlayerAction>,
}

impl Stream for ActionTickStream {
  type Item = Tick;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    // Wait for the delay to be done
    futures::ready!(Pin::new(&mut self.delay).poll(cx));

    let now = self.delay.deadline();

    let delay = (tokio::time::Instant::now() - now).as_millis() as u16;

    let next = now + self.step_duration;
    self.delay.reset(next);

    let tick = Tick {
      time_increment_ms: self.step + delay,
      actions: std::mem::replace(&mut self.actions, vec![]),
    };
    Poll::Ready(Some(tick))
  }
}
