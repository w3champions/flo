use std::time::Instant;

#[derive(Debug, Clone)]
pub struct StopWatch {
  start: Instant,
}

impl StopWatch {
  pub fn new() -> Self {
    StopWatch {
      start: Instant::now(),
    }
  }

  pub fn elapsed_ms(&self) -> u32 {
    self.start.elapsed().as_millis() as u32
  }
}
