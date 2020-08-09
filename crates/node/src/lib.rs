mod echo;
mod game;
mod net;
mod protocol;

pub mod error;

pub enum Status {
  Idle,
}

pub struct ConnectionState {
  pub player_id: i32,
}

pub use self::echo::serve_echo;

pub async fn serve_node() -> crate::error::Result<()> {
  Ok(())
}
