mod connection;

use connection::GamePlayerConn;

#[derive(Debug)]
pub struct Game {
  pub id: i32,
  pub status: GameStatus,
  pub settings: GameSettings,
  pub players: Vec<GamePlayer>,
}

#[derive(Debug)]
pub enum GameStatus {
  Created,
  Started,
  Ended,
}

#[derive(Debug)]
pub struct GameSettings {
  pub map_path: String,
  pub map_checksum: u32,
  pub map_crc32: u32,
  pub map_sha1: [u8; 20],
}

#[derive(Debug)]
pub struct GamePlayer {
  pub id: i32,
  pub name: String,
  pub conn: Option<GamePlayerConn>,
}
