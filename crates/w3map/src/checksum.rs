use crate::error::{Error, Result};
use crate::Archive;
use std::io::{BufReader, Read};

const CHUNK_SIZE: usize = 200 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct MapChecksum {
  pub xoro: u32,
  pub crc32: u32,
  pub sha1: [u8; 20],
  pub file_size: usize,
}

impl MapChecksum {
  pub(crate) fn compute(archive: &mut Archive) -> Result<Self> {
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

    let xoro = {
      let mut xoro = XoroHasher::new();

      let files: &[&[&str]] = &[
        &["war3map.j", "scripts\\war3map.j", "war3map.lua"],
        &["war3map.w3e"],
        &["war3map.wpm"],
        &["war3map.doo"],
        &["war3map.w3u"],
        &["war3map.w3b"],
        &["war3map.w3d"],
        &["war3map.w3a"],
        &["war3map.w3q"],
      ];

      for (i, paths) in files.into_iter().enumerate() {
        let mut found = false;
        for path in *paths {
          if let Some(bytes) = archive.read_file_all_opt(path)? {
            if i == 0 {
              xoro.update(&bytes);
            } else {
              for chunk in bytes.chunks(0x400) {
                if chunk.len() == 0x400 {
                  xoro.update(chunk);
                  xoro.0 = XoroHasher::rol3(xoro.0);
                }
              }
            }
            found = true;
            break;
          }
        }
        if !found && i == 0 {
          return Err(Error::MapScriptNotFound);
        }
      }
      xoro
    };

    Ok(Self {
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

struct XoroHasher(u32);

impl XoroHasher {
  fn new() -> Self {
    XoroHasher(0)
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
