use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Node {
  pub name: String,
  pub location: String,
  pub ip_addr: String,
}
