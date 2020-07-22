use lazy_static::lazy_static;
use std::time::Instant;

lazy_static! {
  static ref INITIAL: Instant = Instant::now();
}

pub fn initialize() {
  lazy_static::initialize(&INITIAL);
}

pub fn uptime_ms() -> u32 {
  Instant::now()
    .checked_duration_since(*INITIAL)
    .map(|d| d.as_millis() as u32)
    .unwrap_or(0)
}

#[test]
fn test_uptime_ms() {
  assert_eq!(uptime_ms(), 0);
  std::thread::sleep(std::time::Duration::from_millis(100));
  assert!(uptime_ms() >= 100);
}

#[test]
fn test_uptime_ms_inited() {
  initialize();
  std::thread::sleep(std::time::Duration::from_millis(100));
  assert!(uptime_ms() >= 100);
  std::thread::sleep(std::time::Duration::from_millis(100));
  assert!(uptime_ms() >= 200);
}
