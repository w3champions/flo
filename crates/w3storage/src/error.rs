use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("invalid path: {0}")]
  InvalidPath(String),
  #[error("platform: {0}")]
  Platform(#[from] flo_platform::error::Error),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("glob pattern error: {0}")]
  GlobPattern(#[from] glob::PatternError),
  #[error("casc: {0}")]
  Casc(#[from] casclib::CascError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
