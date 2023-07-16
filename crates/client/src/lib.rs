mod controller;
pub mod error;
mod game;
mod lan;
mod message;
mod node;
pub mod observer;
mod ping;
pub mod platform;
mod version;
pub use version::FLO_VERSION;

use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct StartConfig {
  pub token: Option<String>,
  pub installation_path: Option<PathBuf>,
  pub user_data_path: Option<PathBuf>,
  pub controller_host: Option<String>,
  pub stats_host: Option<String>,
  pub version: Option<String>,
  pub ptr: Option<bool>,
  pub save_replay: bool, //Default value is false
  pub user_battlenet_client_id: Option<String>,
}

pub use crate::message::embed::{start_embed, FloEmbedClient, FloEmbedClientHandle};
pub use message::messages;

#[cfg(feature = "ws")]
pub use crate::message::ws::{start_ws, FloWsClient};
