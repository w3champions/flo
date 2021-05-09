use once_cell::sync::Lazy;
use rusoto_core::{credential::StaticProvider, request::HttpClient};
use rusoto_kinesis::KinesisClient;
use std::env;

pub static KINESIS_CLIENT: Lazy<KinesisClient> = Lazy::new(|| {
  let provider = StaticProvider::new(
    env::var("AWS_ACCESS_KEY_ID").unwrap(),
    env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
    None,
    None,
  );
  let client = HttpClient::new().unwrap();
  let region = env::var("AWS_KINESIS_REGION").unwrap().parse().unwrap();
  KinesisClient::new_with(client, provider, region)
});