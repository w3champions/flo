use lazy_static::lazy_static;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub secret_key: String,
}

impl Env {
  pub fn get() -> &'static Env {
    lazy_static! {
      static ref INSTANCE: Env = Env {
        secret_key: env::var("FLO_NODE_SECRET").unwrap_or_default()
      };
    }
    &INSTANCE
  }
}
