use rusoto_core::RusotoError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("get archived object: {0}")]
  GetArchivedObject(#[from] RusotoError<rusoto_s3::GetObjectError>),
  #[error("invalid S3 credentials: {0}")]
  InvalidS3Credentials(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
