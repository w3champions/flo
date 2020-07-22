use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
