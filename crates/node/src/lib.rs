mod game;
mod net;
mod protocol;

pub enum Status {
  Idle,
}

pub struct ConnectionState {
  pub player_id: i32,
}
