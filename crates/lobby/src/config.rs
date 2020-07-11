use jsonwebtoken::EncodingKey;
use lazy_static::lazy_static;
use std::env;

lazy_static! {
  pub static ref JWT_SECRET: EncodingKey = EncodingKey::from_base64_secret(
    &env::var("JWT_SECRET_BASE64").expect("env `JWT_SECRET_BASE64`")
  )
  .expect("EncodingKey::from_base64_secret");
}
