use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
  sub: String,
  player_id: i32,
  exp: usize,
}
