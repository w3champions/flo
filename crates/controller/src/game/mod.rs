pub mod db;
mod slots;
pub(crate) mod state;
pub mod token;
mod types;

pub mod messages {
  pub use super::state::cancel::CancelGame;
  pub use super::state::create::CreateGame;
  pub use super::state::join::PlayerJoin;
  pub use super::state::leave::PlayerLeave;
  pub use super::state::node::SelectNode;
  pub use super::state::player::GetGamePlayers;
  pub use super::state::registry::{
    AddGamePlayer, Register, Remove, RemoveGamePlayer, ResolveGamePlayerPingBroadcastTargets,
  };
  pub use super::state::slot::UpdateSlot;
  pub use super::state::start::{StartGameCheck, StartGamePlayerAck};
}

pub use slots::Slots;
pub use types::*;
