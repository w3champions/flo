use std::io::Cursor;
use std::path::{Path, PathBuf};
use stormlib::OpenArchiveFlags;

use flo_util::binary::BinDecode;

pub mod error;

mod checksum;
mod constants;
mod info;
mod minimap;
mod trigger_string;

pub use self::checksum::MapChecksum;
pub use self::constants::*;
pub use self::info::*;
pub use self::minimap::*;
pub use self::trigger_string::*;

pub use flo_blp::BLPImage;
use flo_w3storage::W3Storage;

use self::error::{Error, Result};

#[derive(Debug)]
pub struct W3Map {
  name: String,
  author: String,
  description: String,
  suggested_players: String,
  file_size: usize,
  info: MapInfo,
  image: BLPImage,
  minimap_icons: MinimapIcons,
  trigger_strings: TriggerStringMap,
}

impl W3Map {
  pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
    Self::load_info(Self::open_archive_file(path)?)
  }

  pub fn open_memory(bytes: &[u8]) -> Result<Self> {
    Self::load_info(Self::open_archive_memory(bytes)?)
  }

  pub fn open_storage(storage: &W3Storage, path: &str) -> Result<Self> {
    use flo_w3storage::Data;
    let file = storage
      .resolve_file(path)?
      .ok_or_else(|| Error::StorageFileNotFound(path.to_string()))?;
    match *file.data() {
      Data::Path(ref path) => Self::open(path),
      Data::Bytes(ref bytes) => Self::open_memory(bytes),
    }
  }

  pub fn open_storage_with_checksum(
    storage: &W3Storage,
    path: &str,
  ) -> Result<(Self, MapChecksum)> {
    use flo_w3storage::Data;
    let file = storage
      .resolve_file(path)?
      .ok_or_else(|| Error::StorageFileNotFound(path.to_string()))?;
    let mut archive = match *file.data() {
      Data::Path(ref path) => Self::open_archive_file(path),
      Data::Bytes(ref bytes) => Self::open_archive_memory(bytes),
    }?;
    let checksum = MapChecksum::compute(&storage, &mut archive)?;
    let map = Self::load_info(archive)?;
    Ok((map, checksum))
  }

  pub fn render_preview_png(&self) -> Vec<u8> {
    let mut bg = self.image.clone();
    for icon in self.minimap_icons.iter() {
      icon.draw_into(&mut bg);
    }
    let mut bytes = vec![];
    image::DynamicImage::ImageRgba8(bg)
      .write_to(&mut bytes, image::ImageFormat::Png)
      .ok();
    bytes
  }

  pub fn file_size(&self) -> usize {
    self.file_size
  }

  pub fn name(&self) -> &str {
    self.trigger_strings.get(self.info.name).unwrap_or("")
  }

  pub fn description(&self) -> &str {
    self
      .trigger_strings
      .get(self.info.description)
      .unwrap_or("")
  }
}

pub(crate) fn open_archive<P: AsRef<Path>>(path: P) -> Result<stormlib::Archive> {
  stormlib::Archive::open(
    path,
    OpenArchiveFlags::MPQ_OPEN_NO_LISTFILE
      | OpenArchiveFlags::MPQ_OPEN_NO_ATTRIBUTES
      | OpenArchiveFlags::STREAM_FLAG_READ_ONLY,
  )
  .map_err(Into::into)
}

impl W3Map {
  fn open_archive_file<'a, P: AsRef<Path>>(path: P) -> Result<Archive<'a>> {
    Ok(Archive::File(FileArchive {
      path: path.as_ref().to_owned(),
      inner: open_archive(path)?,
    }))
  }

  fn open_archive_memory(bytes: &[u8]) -> Result<Archive> {
    Ok(Archive::Memory(MemoryArchive {
      bytes,
      inner: ceres_mpq::Archive::open(Cursor::new(bytes))?,
    }))
  }

  fn load_info(mut archive: Archive) -> Result<Self> {
    let info: MapInfo = {
      let bytes = archive.read_file_all("war3map.w3i")?;
      BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadInfo)?
    };
    let trigger_strings = {
      let bytes = archive.read_file_all("war3map.wts")?;
      TriggerStringMap::decode(&mut bytes.as_slice()).map_err(Error::ReadTriggerStrings)?
    };

    Ok(W3Map {
      name: trigger_strings
        .get(info.name)
        .map(ToString::to_string)
        .unwrap_or_else(|| "".to_string()),
      author: trigger_strings
        .get(info.author)
        .map(ToString::to_string)
        .unwrap_or_else(|| "".to_string()),
      description: trigger_strings
        .get(info.description)
        .map(ToString::to_string)
        .unwrap_or_else(|| "".to_string()),
      suggested_players: trigger_strings
        .get(info.suggested_players)
        .map(ToString::to_string)
        .unwrap_or_else(|| "".to_string()),
      file_size: archive.get_size()?,
      info,
      image: {
        let bytes = archive.read_file_all("war3mapMap.blp")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadImage)?
      },
      minimap_icons: {
        let bytes = archive.read_file_all("war3map.mmp")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadMinimapIcons)?
      },
      trigger_strings,
    })
  }
}

struct FileArchive {
  path: PathBuf,
  inner: stormlib::Archive,
}
struct MemoryArchive<'a> {
  bytes: &'a [u8],
  inner: ceres_mpq::Archive<Cursor<&'a [u8]>>,
}

pub(crate) enum Archive<'a> {
  File(FileArchive),
  Memory(MemoryArchive<'a>),
}

impl<'a> Archive<'a> {
  fn get_size(&mut self) -> Result<usize> {
    match *self {
      Archive::File(ref mut archive) => {
        let meta = std::fs::metadata(&archive.path)?;
        Ok(meta.len() as usize)
      }
      Archive::Memory(ref mut archive) => Ok(archive.bytes.len()),
    }
  }

  fn read_file_all(&mut self, path: &str) -> Result<Vec<u8>> {
    let bytes = match *self {
      Archive::File(ref mut archive) => archive.inner.open_file(path)?.read_all()?,
      Archive::Memory(ref mut archive) => archive.inner.read_file(path)?,
    };
    Ok(bytes)
  }

  fn read_file_all_opt(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
    self.read_file_all(path).map(Some).or_else(|e| {
      if Self::is_err_file_not_found(&e) {
        Ok(None)
      } else {
        Err(e)
      }
    })
  }

  fn is_err_file_not_found(e: &Error) -> bool {
    match *e {
      Error::Storm(stormlib::error::StormError::FileNotFound) => true,
      Error::CeresMpq(ceres_mpq::Error::FileNotFound) => true,
      _ => false,
    }
  }
}

#[test]
fn test_open_map() {
  for name in &[
    "(2)ConcealedHill.w3x",
    "(8)Sanctuary_LV.w3x",
    "test_roc.w3m",
    "test_tft.w3x",
    "(4)adrenaline.w3m",
  ] {
    let map = W3Map::open(flo_util::sample_path!("map", name)).unwrap();
    // let _data = map.render_preview_png();
    // std::fs::write(format!("{}.png", name), data).unwrap()
    dbg!(map);
  }
}

#[test]
fn test_open_storage() {
  let storage = W3Storage::from_env().unwrap();
  let _map = W3Map::open_storage(&storage, "maps\\(4)adrenaline.w3m").unwrap();
  std::fs::write("adrenaline.png", _map.render_preview_png()).unwrap()
}

#[test]
#[ignore] // xoro doesn't work
fn test_open_storage_with_checksum() {
  let storage = W3Storage::from_env().unwrap();
  let (_map, checksum) =
    W3Map::open_storage_with_checksum(&storage, "maps\\(2)bootybay.w3m").unwrap();

  assert_eq!(
    checksum,
    MapChecksum {
      xoro: 2039165270,
      crc32: 1444344839,
      sha1: [
        201, 228, 110, 214, 86, 255, 142, 141, 140, 96, 141, 57, 3, 110, 63, 27, 250, 11, 28, 194,
      ],
    }
  )
}
