mod kinesis;
pub mod record;
pub mod error;
pub mod token;

use once_cell::sync::Lazy;

pub use kinesis::KINESIS_CLIENT;
pub static KINESIS_STREAM_NAME: Lazy<String> = Lazy::new(|| {
  std::env::var("AWS_KINESIS_STREAM_NAME")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or("flo".to_string())
});
