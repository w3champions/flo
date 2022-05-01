use flo_observer::record::ObserverRecordSource;
use once_cell::sync::Lazy;
use std::env;

#[derive(Debug)]
pub struct Env {
  pub controller_url: String,
  pub controller_secret: String,
  pub record_source: ObserverRecordSource,
  pub record_backscan_secs: u64,
  pub jwt_secret_base64: String,
  pub aws_s3_region: Option<String>,
  pub aws_s3_bucket: Option<String>,
  pub aws_access_key_id: Option<String>,
  pub aws_secret_access_key: Option<String>,
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
    record_backscan_secs: std::env::var("OBSERVER_BACKSCAN_SECS")
      .ok()
      .and_then(|v| v.parse().ok())
      .unwrap_or(3600),
    aws_s3_region: env::var("AWS_S3_REGION").ok(),
    aws_s3_bucket: env::var("AWS_S3_BUCKET").ok(),
    aws_access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
    aws_secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
  }
});
