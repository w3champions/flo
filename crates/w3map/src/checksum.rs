use std::io::{BufReader, Read};

use flo_w3storage::W3Storage;

use crate::error::Result;
use crate::Archive;

const CHUNK_SIZE: usize = 200 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct MapChecksum {
  pub crc32: u32,
  pub sha1: [u8; 20],
  pub file_size: usize,
}

impl MapChecksum {
  pub(crate) fn compute(_storage: &W3Storage, archive: &mut Archive) -> Result<Self> {
    let mut sha1 = sha1::Sha1::new();
    let mut crc32 = crc32fast::Hasher::new();
    let file_size;

    match *archive {
      Archive::File(ref mut archive) => {
        let mut buf = Vec::with_capacity(CHUNK_SIZE);
        buf.resize(CHUNK_SIZE, 0);
        let file = std::fs::File::open(&archive.path)?;
        file_size = file.metadata()?.len() as usize;
        let mut r = BufReader::new(file);
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
        file_size = archive.bytes.len();
        sha1.update(archive.bytes);
        crc32.update(archive.bytes);
      }
    }

    Ok(Self {
      crc32: crc32.finalize(),
      sha1: sha1.digest().bytes(),
      file_size,
    })
  }

  pub fn get_sha1_hex_string(&self) -> String {
    self.sha1.iter().map(|b| format!("{:02x}", b)).collect()
  }
}
