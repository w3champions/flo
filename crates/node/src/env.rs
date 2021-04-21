use once_cell::sync::Lazy;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub secret_key: String,
}

impl Env {
  pub fn get() -> &'static Env {
    static INSTANCE: Lazy<Env> = Lazy::new(|| Env {
      secret_key: env::var("FLO_NODE_SECRET").unwrap_or_default(),
    });
    &INSTANCE
  }
}
