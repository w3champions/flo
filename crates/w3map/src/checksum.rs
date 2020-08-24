use std::io::{BufReader, Read};

use flo_w3storage::W3Storage;

use crate::error::Result;
use crate::Archive;

const CHUNK_SIZE: usize = 200 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct MapChecksum {
  #[cfg(feature = "xoro")]
  pub xoro: u32,
  pub crc32: u32,
  pub sha1: [u8; 20],
  pub file_size: usize,
}

impl MapChecksum {
  //TODO figure out the new xoro checksum algorithm
  pub(crate) fn compute(storage: &W3Storage, archive: &mut Archive) -> Result<Self> {
    #[cfg(not(feature = "xoro"))]
    let _ = storage;

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

    #[cfg(feature = "xoro")]
    let xoro = {
      let mut xoro = XoroHasher::new();

      for path in &["scripts\\common.j", "scripts\\blizzard.j"] {
        if let Some(bytes) = archive.read_file_all_opt(path)? {
          xoro.0 = xoro.0 ^ XoroHasher::all(&bytes);
        } else {
          let bytes = storage
            .resolve_file(path)?
            .ok_or_else(|| crate::error::Error::StorageFileNotFound(path.to_string()))?
            .read_all()?;
          xoro.0 = xoro.0 ^ XoroHasher::all(bytes.as_ref());
        }
      }

      xoro.0 = XoroHasher::rol3(xoro.0);

      xoro.update(&[0x9E, 0x37, 0xF1, 0x03]);

      let files: &[&[&str]] = &[
        &["war3map.j", "scripts\\war3map.j"],
        &["war3map.w3e"],
        &["war3map.wpm"],
        &["war3map.doo"],
        &["war3map.w3u"],
        &["war3map.w3b"],
        &["war3map.w3d"],
        &["war3map.w3a"],
        &["war3map.w3q"],
      ];

      for paths in files {
        for path in *paths {
          if let Some(bytes) = archive.read_file_all_opt(path)? {
            xoro.update(&XoroHasher::all(&bytes).to_le_bytes());
            break;
          } else {
            if let Some(bytes) = storage
              .resolve_file(path)?
              .map(|mut f| f.read_all())
              .transpose()?
            {
              xoro.update(&XoroHasher::all(&bytes).to_le_bytes());
              break;
            }
          }
        }
      }
    };

    Ok(Self {
      #[cfg(feature = "xoro")]
      xoro: xoro.finalize(),
      crc32: crc32.finalize(),
      sha1: sha1.digest().bytes(),
      file_size,
    })
  }

  pub fn get_sha1_hex_string(&self) -> String {
    self.sha1.iter().map(|b| format!("{:02x}", b)).collect()
  }
}

#[cfg(feature = "xoro")]
struct XoroHasher(u32);

#[cfg(feature = "xoro")]
impl XoroHasher {
  fn new() -> Self {
    XoroHasher(0)
  }

  fn all(bytes: &[u8]) -> u32 {
    let mut hasher = XoroHasher::new();
    hasher.update(bytes);
    hasher.finalize()
  }

  fn rol3(v: u32) -> u32 {
    v << 3 | v >> 29
  }

  fn update(&mut self, bytes: &[u8]) {
    let mut v = self.0;
    for chunk in bytes.chunks(4) {
      if chunk.len() == 4 {
        v = v ^ u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        v = Self::rol3(v)
      } else {
        for byte in chunk {
          v = v ^ (*byte as u32);
          v = Self::rol3(v)
        }
      }
    }
    self.0 = v;
  }

  fn finalize(self) -> u32 {
    self.0
  }
}
