pub mod path_tree;

use bytes::Bytes;
use casclib::Storage;
use glob::Pattern;
use parking_lot::Mutex;
use std::path::PathBuf;

use flo_platform::ClientPlatformInfo;

pub mod error;

use error::*;

#[derive(Debug)]
pub struct W3Storage {
  storage_path: PathBuf,
  overrides: Vec<OverridePath>,
  handle: Mutex<Option<Storage>>,
}

impl W3Storage {
  pub fn new(platform: &ClientPlatformInfo) -> Result<Self> {
    let mut inst = Self {
      storage_path: platform.installation_path.join("Data"),
      overrides: vec![],
      handle: Mutex::new(None),
    };

    inst.add_override("maps/**/*", platform.user_data_path.clone())?;

    Ok(inst)
  }

  pub fn from_env() -> Result<Self> {
    let platform = ClientPlatformInfo::from_env()?;
    Self::new(&platform)
  }

  pub fn add_override(&mut self, pattern: &str, path: PathBuf) -> Result<()> {
    self.overrides.push(OverridePath {
      pattern: Pattern::new(pattern)?,
      path,
    });
    Ok(())
  }

  pub fn list_storage_files(&self, mask: &str) -> Result<Vec<String>> {
    let mask = Self::get_storage_path(mask);
    self
      .with_storage(|s| -> Result<_, casclib::CascError> {
        use std::iter::FromIterator;
        Result::<_, casclib::CascError>::from_iter(
          s.files_with_mask(mask)
            .into_iter()
            .map(|f| f.map(|f| f.get_name().to_string())),
        )
      })?
      .map_err(Into::into)
  }

  pub fn resolve_file(&self, path: &str) -> Result<Option<File>> {
    let lower = path.to_lowercase();
    let overrides = self.find_overrides(&lower);
    if !overrides.is_empty() {
      for base in overrides {
        let resolved_path = base.join(path);
        match std::fs::metadata(&resolved_path) {
          Ok(m) => {
            return Ok(Some(File {
              source: FileSource::Override,
              size: m.len(),
              data: Data::Path(resolved_path),
            }))
          }
          Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
            continue;
          }
          Err(e) => return Err(e.into()),
        }
      }
    }
    let bytes = self.with_storage(|s| -> Result<_, casclib::CascError> {
      s.entry(&Self::get_storage_path(path))
        .open()
        .and_then(|e| e.read_all())
        .map(|bytes| {
          Some(File {
            source: FileSource::Storage,
            size: bytes.len() as u64,
            data: Data::Bytes(Bytes::from(bytes)),
          })
        })
        .or_else(|e| match e {
          casclib::CascError::FileNotFound => Ok(None),
          e => Err(e),
        })
    })??;
    Ok(bytes)
  }

  fn get_storage_path(path: &str) -> String {
    format!("war3.w3mod:{}", path)
  }

  fn find_overrides(&self, path: &str) -> Vec<PathBuf> {
    self
      .overrides
      .iter()
      .filter_map(|o| {
        if o.pattern.matches(path) {
          Some(o.path.clone())
        } else {
          None
        }
      })
      .collect()
  }

  fn with_storage<F, R>(&self, f: F) -> Result<R>
  where
    F: FnOnce(&Storage) -> R,
  {
    let mut lock = self.handle.lock();
    if let Some(storage) = lock.as_ref() {
      Ok(f(storage))
    } else {
      let storage = casclib::open(&self.storage_path)?;
      let r = f(&storage);
      *lock = Some(storage);
      Ok(r)
    }
  }
}

#[derive(Debug)]
pub struct File {
  source: FileSource,
  size: u64,
  data: Data,
}

impl File {
  pub fn source(&self) -> FileSource {
    self.source
  }

  pub fn size(&self) -> u64 {
    self.size
  }

  pub fn data(&self) -> &Data {
    &self.data
  }

  pub fn read_all(&mut self) -> Result<Bytes> {
    self.data.get_bytes()
  }
}

#[derive(Debug, Copy, Clone)]
pub enum FileSource {
  Override,
  Storage,
}

#[derive(Debug)]
pub enum Data {
  Path(PathBuf),
  Bytes(Bytes),
}

impl Data {
  fn get_bytes(&mut self) -> Result<Bytes> {
    let (replace, bytes) = match *self {
      Data::Path(ref path) => {
        let bytes = Bytes::from(std::fs::read(path)?);
        (Some(bytes.clone()), bytes)
      }
      Data::Bytes(ref bytes) => (None, bytes.clone()),
    };
    if let Some(replace) = replace {
      *self = Data::Bytes(replace);
    }
    Ok(bytes)
  }
}

#[derive(Debug)]
pub struct OverridePath {
  pattern: Pattern,
  path: PathBuf,
}

#[test]
fn test_storage() {
  let p = ClientPlatformInfo::from_env().unwrap();
  let mut s = W3Storage::new(&p).unwrap();
  let crate_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
  s.add_override("cargo*", crate_dir.clone()).unwrap();
  assert!(s.resolve_file("___SHOULD_NOT_EXIST").unwrap().is_none());
  assert_eq!(
    s.resolve_file("Cargo.toml")
      .unwrap()
      .unwrap()
      .read_all()
      .unwrap(),
    std::fs::read(crate_dir.join("Cargo.toml")).unwrap()
  );
  assert!(s.resolve_file("scripts/common.j").unwrap().is_some());
}
