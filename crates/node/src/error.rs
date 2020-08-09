use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("tokio io: {0}")]
  Tokio(#[from] tokio::io::Error),
  #[error("operation timeout")]
  Timeout(#[from] tokio::time::Elapsed),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
