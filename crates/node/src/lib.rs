mod echo;
mod env;
mod game;
mod metrics;
mod net;
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
  let mut handler = net::ControllerHandler::new();

  handler.serve().await?;

  Ok(())
}

pub async fn serve_client() -> Result<()> {
  Ok(())
}
