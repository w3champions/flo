use flo_observer::record::ObserverRecordSource;
use once_cell::sync::Lazy;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub redis_url: String,
  pub record_source: ObserverRecordSource,
}

pub static ENV: Lazy<Env> = Lazy::new(|| Env {
  redis_url: env::var("REDIS_URL").expect("env REDIS_URL"),
  record_source: std::env::var("OBSERVER_CONSUMER_SOURCE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(ObserverRecordSource::Test),
});
