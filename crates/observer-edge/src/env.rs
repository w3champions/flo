use flo_observer::record::ObserverRecordSource;
use once_cell::sync::Lazy;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub controller_url: String,
  pub controller_secret: String,
  pub record_source: ObserverRecordSource,
  pub jwt_secret_base64: String,
}

pub static ENV: Lazy<Env> = Lazy::new(|| {
  Env {
    controller_url: env::var("CONTROLLER_URL").expect("env CONTROLLER_URL"),
    controller_secret: env::var("CONTROLLER_SECRET").expect("env CONTROLLER_SECRET"),
    record_source: std::env::var("OBSERVER_CONSUMER_SOURCE")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(ObserverRecordSource::Test),
    jwt_secret_base64: env::var("JWT_SECRET_BASE64").expect("env JWT_SECRET_BASE64"),
  }
});
