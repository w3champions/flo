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
    Instant::now()
      .checked_duration_since(self.start)
      .map(|d| d.as_millis() as u32)
      .unwrap_or_default()
  }
}
