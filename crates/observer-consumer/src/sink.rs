use flo_observer::record::GameRecord;
use tokio::sync::mpsc::channel;
use tokio::fs::{self, File};
use crate::error::Result;
use crate::cache::Cache;
use serde::{Serialize, Deserialize};

pub struct GameSink {
  game_id: i32,
  cache: Cache,
  data_file: File,
}

impl GameSink {
  pub async fn open(game_id: i32) -> Result<Self> {
    todo!()
  }
}

pub enum PushResult {
  Incomplete,
  Continue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameStreamMeta {
  pub id: i32,
  pub archive_url: Option<String>,
  pub error_message: Option<String>,
}