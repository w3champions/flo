#[cfg(not(debug_assertions))]
#[macro_use]
extern crate diesel_migrations;
#[cfg(not(debug_assertions))]
pub mod migration;

#[macro_use]
mod macros;
mod version;

#[macro_use]
extern crate diesel;

mod db;
mod schema;

mod config;
mod connect;
pub mod error;
pub mod game;
mod grpc;
pub mod host;
pub mod map;
pub mod node;
pub mod player;
mod state;

pub use connect::serve as serve_socket;
pub use grpc::serve as serve_grpc;
pub use state::ControllerStateRef;
