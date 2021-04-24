use chrono::Utc;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::error::*;

const TOKEN_EXPIRATION_SECS: i64 = 15 * 60;
const TOKEN_SUB: &str = "flo";

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinToken {
  pub sub: String,
  pub game_id: i32,
  pub exp: usize,
}

pub fn create_join_token(game_id: i32) -> Result<String> {
  static ENCODING_KEY: Lazy<EncodingKey> = Lazy::new(|| {
    EncodingKey::from_base64_secret(&crate::config::JWT_SECRET_BASE64)
      .expect("DecodingKey::from_base64_secret")
  });

  let exp = Utc::now().timestamp() + TOKEN_EXPIRATION_SECS;
  let claims = JoinToken {
    sub: TOKEN_SUB.to_string(),
    game_id,
    exp: exp as usize,
  };
  encode(&Header::default(), &claims, &ENCODING_KEY).map_err(Into::into)
}

pub fn validate_join_token(token: &str) -> Result<JoinToken> {
  let decoding_key = DecodingKey::from_base64_secret(&crate::config::JWT_SECRET_BASE64)?;
  decode(token, &decoding_key, &Validation::default())
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
      ErrorKind::ExpiredSignature => Error::JoinTokenExpired,
      _ => e.into(),
    })
}

#[test]
fn test_join_token() {
  dotenv::dotenv().unwrap();
  let token = create_join_token(100).unwrap();
  let token = validate_join_token(&token).unwrap();
  dbg!(token);
}
