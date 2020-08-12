use lazy_static::lazy_static;
use prometheus::{register_int_gauge, IntGauge};

lazy_static! {
  pub static ref PENDING_PLAYER_TOKENS: IntGauge = register_int_gauge!(
    "flonode_pending_player_tokens",
    "Number of pending player tokens"
  )
  .unwrap();
  pub static ref CONNECTED_PLAYERS: IntGauge =
    register_int_gauge!("flonode_connected_players", "Number of players connected").unwrap();
  pub static ref GAME_SESSIONS: IntGauge =
    register_int_gauge!("flonode_game_sessions", "Number of game sessions").unwrap();
}
