use once_cell::sync::Lazy;

pub static ENV: Lazy<Env> = Lazy::new(|| {
  let controller_host = std::env::var("FLO_CONTROLLER_HOST")
    .ok()
    .unwrap_or_else(|| "127.0.0.1".to_string());
  let controller_secret = std::env::var("FLO_CONTROLLER_SECRET")
    .ok()
    .unwrap_or_else(|| "TEST".to_string());
  let stats_host = std::env::var("FLO_STATS_HOST")
    .ok()
    .unwrap_or_else(|| "127.0.0.1".to_string());
  Env {
    controller_host,
    controller_secret,
    stats_host,
  }
});

pub struct Env {
  pub controller_host: String,
  pub controller_secret: String,
  pub stats_host: String,
}
