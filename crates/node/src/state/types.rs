use uuid::Uuid;

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
pub struct PlayerToken([u8; 16]);

impl PlayerToken {
  pub fn new_uuid() -> Self {
    let uuid = Uuid::new_v4();
    Self(*uuid.as_bytes())
  }

  pub fn from_vec(bytes: Vec<u8>) -> Option<Self> {
    if bytes.len() != 16 {
      return None;
    }
    let mut token = PlayerToken([0; 16]);
    token.0.copy_from_slice(&bytes[..]);
    Some(token)
  }

  pub fn to_vec(&self) -> Vec<u8> {
    self.0.to_vec()
  }
}

#[derive(Debug, Clone)]
pub struct RegisteredPlayer {
  pub player_id: i32,
  pub game_id: i32,
}
