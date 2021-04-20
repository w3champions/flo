use lazy_static::lazy_static;
use std::time::Duration;

lazy_static! {
  pub static ref ENV_GAME_STEP_MS: Option<u16> = {
    std::env::var("FLO_GAME_STEP_MS")
      .ok()
      .and_then(|v| v.parse().ok())
  };
}

pub const PEER_CHANNEL_SIZE: usize = 250;
pub const CONTROLLER_SENDER_BUF_SIZE: usize = 10;
pub const GAME_DISPATCH_BUF_SIZE: usize = 256;
pub const GAME_PLAYER_LAGGING_THRESHOLD_MS: u32 = 1000;
pub const GAME_DEFAULT_STEP_MS: u16 = 20;
pub const GAME_PING_INTERVAL: Duration = Duration::from_secs(1);
pub const GAME_PING_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(not(debug_assertions))]
pub const GAME_DELAY_RANGE: [Duration; 2] = [Duration::from_millis(25), Duration::from_millis(100)];
#[cfg(debug_assertions)]
pub const GAME_DELAY_RANGE: [Duration; 2] =
  [Duration::from_millis(25), Duration::from_millis(60 * 1000)];
