#[macro_use]
extern crate diesel;

mod db;
mod schema;

pub mod api_client;
mod config;
mod connect;
pub mod error;
pub mod game;
pub mod host;
pub mod map;
pub mod node;
pub mod player;
mod service;
mod state;

pub use api_client::Storage as ApiClientStorage;
pub use service::FloLobbyService;
pub use state::Storage as StateStorage;
