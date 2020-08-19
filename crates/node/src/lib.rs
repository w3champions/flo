mod client;
mod controller;
mod echo;
mod env;
mod game;
mod metrics;
mod session;
mod version;

pub mod error;
use error::Result;

pub enum Status {
  Idle,
}

pub struct ConnectionState {
  pub player_id: i32,
}

pub use self::echo::serve_echo;

pub async fn serve_controller() -> Result<()> {
  let mut handler = controller::ControllerHandler::new();

  handler.serve().await?;

  Ok(())
}

pub use self::client::serve_client;
pub use self::metrics::serve_metrics;
