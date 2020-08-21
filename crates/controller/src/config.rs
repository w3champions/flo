use lazy_static::lazy_static;
use std::env;

lazy_static! {
  pub static ref JWT_SECRET_BASE64: String =
    env::var("JWT_SECRET_BASE64").expect("env `JWT_SECRET_BASE64`");
}
