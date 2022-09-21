use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("flo observer slot occupied")]
  FloObserverSlotOccupied,
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("observer fs: {0}")]
  ObserverFs(#[from] flo_observer_fs::error::Error),
  #[error("w3replay: {0}")]
  W3Replay(#[from] flo_w3replay::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
