use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("redis: {0}")]
  Redis(#[from] redis::RedisError),
}

pub type Result<T> = std::result::Result<T, Error>;