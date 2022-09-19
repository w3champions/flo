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
    aws_s3_region: std::env::var("AWS_S3_REGION").ok(),
    aws_s3_bucket: std::env::var("AWS_S3_BUCKET").ok(),
    aws_access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
    aws_secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
  }
});

pub struct Env {
  pub controller_host: String,
  pub controller_secret: String,
  pub stats_host: String,
  pub aws_s3_region: Option<String>,
  pub aws_s3_bucket: Option<String>,
  pub aws_access_key_id: Option<String>,
  pub aws_secret_access_key: Option<String>,
}
