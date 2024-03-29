use chrono::Utc;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::error::*;

// 1 month
const TOKEN_EXPIRATION_SECS: i64 = 3600 * 24 * 30;
const TOKEN_SUB: &str = "flo";

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerToken {
  pub sub: String,
  pub player_id: i32,
  pub exp: usize,
}

pub fn create_player_token(player_id: i32) -> Result<String> {
  static ENCODING_KEY: Lazy<EncodingKey> = Lazy::new(|| {
    EncodingKey::from_base64_secret(&crate::config::JWT_SECRET_BASE64)
      .expect("DecodingKey::from_base64_secret")
  });

  let exp = Utc::now().timestamp() + TOKEN_EXPIRATION_SECS;
  let claims = PlayerToken {
    sub: TOKEN_SUB.to_string(),
    player_id,
    exp: exp as usize,
  };
  encode(&Header::default(), &claims, &ENCODING_KEY).map_err(Into::into)
}

pub fn validate_player_token(token: &str) -> Result<PlayerToken> {
  let decoding_key = DecodingKey::from_base64_secret(&crate::config::JWT_SECRET_BASE64)?;
  decode(token, &decoding_key, &Validation::default())
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
      ErrorKind::ExpiredSignature => Error::PlayerTokenExpired,
      _ => e.into(),
    })
}

#[test]
fn test_player_token() {
  dotenv::dotenv().unwrap();
  let token = create_player_token(100).unwrap();
  let token = validate_player_token(&token).unwrap();
  dbg!(token);
}
