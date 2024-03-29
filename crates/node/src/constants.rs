use flo_observer::record::ObserverRecordSource;
use once_cell::sync::Lazy;
use std::time::Duration;

pub const PEER_CHANNEL_SIZE: usize = 250;
pub const CONTROLLER_SENDER_BUF_SIZE: usize = 10;
pub const GAME_DISPATCH_BUF_SIZE: usize = 256;
pub const GAME_PLAYER_LAGGING_THRESHOLD_MS: u32 = 3000;
pub const GAME_PLAYER_MAX_ACK_QUEUE: usize = 300;
pub static GAME_DEFAULT_STEP_MS: Lazy<u16> = Lazy::new(|| {
  std::env::var("FLO_GAME_STEP_MS")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(30)
});
pub const GAME_PING_INTERVAL: Duration = Duration::from_secs(1);
pub const GAME_PING_TIMEOUT: Duration = Duration::from_secs(5);
pub const GAME_CLOCK_MAX_PAUSE: Duration = Duration::from_secs(60 - 3);

#[cfg(not(debug_assertions))]
pub const GAME_DELAY_RANGE: [Duration; 2] = [Duration::from_millis(25), Duration::from_millis(100)];
#[cfg(debug_assertions)]
pub const GAME_DELAY_RANGE: [Duration; 2] =
  [Duration::from_millis(25), Duration::from_millis(60 * 1000)];

pub const OBS_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
pub const OBS_CHANNEL_SIZE: usize = 10000;
pub const OBS_MAX_CHUNK_SIZE: usize = 512 * 1024;
pub static OBS_SOURCE: Lazy<ObserverRecordSource> = Lazy::new(|| {
  std::env::var("OBSERVER_SOURCE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(ObserverRecordSource::Test)
});

pub const RTT_STATS_REPORT_DELAY: Duration = std::time::Duration::from_secs(5);
pub const RTT_STATS_REPORT_INTERVAL: Duration = std::time::Duration::from_secs(15);
