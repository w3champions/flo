use std::env;
use once_cell::sync::Lazy;

#[derive(Debug)]
pub struct Env {
  pub redis_url: String,
}

pub static ENV: Lazy<Env> = Lazy::new(|| {
  Env {
    redis_url: env::var("REDIS_URL").expect("env REDIS_URL")
  }
});

