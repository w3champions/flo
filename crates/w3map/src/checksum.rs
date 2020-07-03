use std::io::{BufReader, Read};

use crate::error::Result;
use crate::Archive;

use flo_w3storage::{Data, File, W3Storage};

const CHUNK_SIZE: usize = 200 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct MapChecksum {
  pub xoro: u32,
  pub crc32: u32,
  pub sha1: [u8; 20],
}

impl MapChecksum {
  pub(crate) fn compute(_storage: &W3Storage, archive: &mut Archive) -> Result<Self> {
    let mut sha1 = sha1::Sha1::new();
    let mut crc32 = crc32fast::Hasher::new();

    match *archive {
      Archive::File(ref mut archive) => {
        let mut buf = Vec::with_capacity(CHUNK_SIZE);
        buf.resize(CHUNK_SIZE, 0);
        let mut r = BufReader::new(std::fs::File::open(&archive.path)?);
        loop {
          let len = r.read(&mut buf)?;
          if len == 0 {
            break;
          } else {
            sha1.update(&buf[..len]);
            crc32.update(&buf[..len]);
          }
        }
      }
      Archive::Memory(ref mut archive) => {
        sha1.update(archive.bytes);
        crc32.update(archive.bytes);
      }
    }
    Ok(Self {
      xoro: 0xFFFFFFFF,
      crc32: crc32.finalize(),
      sha1: sha1.digest().bytes(),
    })
  }
}
