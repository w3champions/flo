use std::path::Path;
use stormlib::{Archive, OpenArchiveFlags};

pub mod error;
pub mod info;
pub mod trigger_string;
pub mod constants;

use self::error::Result;

pub struct W3Map {
  pub(crate) archive: Archive,
}

impl W3Map {
  pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
    let archive = Archive::open(
      path,
      OpenArchiveFlags::MPQ_OPEN_NO_LISTFILE | OpenArchiveFlags::MPQ_OPEN_NO_ATTRIBUTES,
    )?;
    Ok(W3Map { archive })
  }
}
