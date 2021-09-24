use chrono::Utc;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::error::*;

// 15mins
const TOKEN_EXPIRATION_SECS: i64 = 60 * 15;
const TOKEN_SUB: &str = "flo-observer";

#[derive(Debug, Serialize, Deserialize)]
pub struct ObserverToken {
  pub sub: String,
  pub game_id: i32,
  pub delay_secs: Option<usize>,
  pub exp: usize,
}

pub fn create_observer_token(game_id: i32, delay_secs: Option<usize>) -> Result<String> {
  static ENCODING_KEY: Lazy<EncodingKey> = Lazy::new(|| {
    EncodingKey::from_base64_secret(&crate::env::ENV.jwt_secret_base64)
      .expect("DecodingKey::from_base64_secret")
  });

  let exp = Utc::now().timestamp() + TOKEN_EXPIRATION_SECS;
  let claims = ObserverToken {
    sub: TOKEN_SUB.to_string(),
    game_id,
    delay_secs,
    exp: exp as usize,
  };
  encode(&Header::default(), &claims, &ENCODING_KEY).map_err(Into::into)
}

pub fn validate_observer_token(token: &str) -> Result<ObserverToken> {
  let decoding_key = DecodingKey::from_base64_secret(&crate::env::ENV.jwt_secret_base64)?;
  decode(token, &decoding_key, &Validation::default())
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
      ErrorKind::ExpiredSignature => Error::ObserverTokenExpired,
      _ => e.into(),
    })
}
