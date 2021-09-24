use flo_observer::record::ObserverRecordSource;
use once_cell::sync::Lazy;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub redis_url: String,
  pub record_source: ObserverRecordSource,
  pub jwt_secret_base64: String,
}

pub static ENV: Lazy<Env> = Lazy::new(|| Env {
  redis_url: env::var("REDIS_URL").expect("env REDIS_URL"),
  record_source: std::env::var("OBSERVER_CONSUMER_SOURCE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(ObserverRecordSource::Test),
  jwt_secret_base64: env::var("JWT_SECRET_BASE64").expect("env JWT_SECRET_BASE64"),
});
