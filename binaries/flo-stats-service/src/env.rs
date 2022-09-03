use once_cell::sync::Lazy;
use std::env;

pub static ADMIN_SECRET: Lazy<String> =
  Lazy::new(|| env::var("FLO_ADMIN_SECRET").ok().unwrap_or_default());
