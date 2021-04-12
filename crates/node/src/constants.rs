use lazy_static::lazy_static;
use std::time::Duration;

lazy_static! {
  pub static ref ENV_GAME_STEP_MS: Option<u16> = {
    std::env::var("FLO_GAME_STEP_MS")
      .ok()
      .and_then(|v| v.parse().ok())
  };
}

pub const PEER_COMMAND_CHANNEL_SIZE: usize = 3;
pub const PEER_W3GS_CHANNEL_SIZE: usize = 250;
pub const CONTROLLER_SENDER_BUF_SIZE: usize = 10;
pub const GAME_DISPATCH_BUF_SIZE: usize = 100;
// pub const GAME_PLAYER_LAGGING_THRESHOLD: usize = 10;
pub const GAME_DEFAULT_STEP_MS: u16 = 20;
pub const GAME_PING_INTERVAL: Duration = Duration::from_secs(15);
pub const GAME_PING_TIMEOUT: Duration = Duration::from_secs(10);
