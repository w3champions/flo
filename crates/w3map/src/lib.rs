use flo_util::binary::BinDecode;
use std::io::Cursor;
use std::path::Path;
use stormlib::OpenArchiveFlags;

type FileArchive = stormlib::Archive;
type MemoryArchive<'a> = ceres_mpq::Archive<Cursor<&'a [u8]>>;

pub mod error;

mod constants;
mod info;
mod minimap;
mod trigger_string;

pub use self::constants::*;
pub use self::info::*;
pub use self::minimap::*;
pub use self::trigger_string::*;

pub use flo_blp::BLPImage;
use flo_w3storage::W3Storage;

use self::error::{Error, Result};

#[derive(Debug)]
pub struct W3Map {
  info: MapInfo,
  image: BLPImage,
  minimap_icons: MinimapIcons,
  trigger_strings: TriggerStringMap,
}

impl W3Map {
  pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
    Self::load(Archive::File(open_archive(path)?))
  }

  pub fn open_memory(bytes: &[u8]) -> Result<Self> {
    Self::load(Archive::Memory(MemoryArchive::open(Cursor::new(bytes))?))
  }

  pub fn open_storage(storage: &W3Storage, path: &str) -> Result<Self> {
    use flo_w3storage::Data;
    let file = storage
      .resolve_file(path)?
      .ok_or_else(|| Error::StorageFileNotFound)?;
    match *file.data() {
      Data::Path(ref path) => Self::open(path),
      Data::Bytes(ref bytes) => Self::open_memory(bytes),
    }
  }

  fn load(mut archive: Archive) -> Result<Self> {
    Ok(W3Map {
      info: {
        let bytes = archive.read_file_all("war3map.w3i")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadInfo)?
      },
      image: {
        let bytes = archive.read_file_all("war3mapMap.blp")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadImage)?
      },
      minimap_icons: {
        let bytes = archive.read_file_all("war3map.mmp")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadMinimapIcons)?
      },
      trigger_strings: {
        let bytes = archive.read_file_all("war3map.wts")?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadTriggerStrings)?
      },
    })
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
}

pub(crate) fn open_archive<P: AsRef<Path>>(path: P) -> Result<FileArchive> {
  stormlib::Archive::open(
    path,
    OpenArchiveFlags::MPQ_OPEN_NO_LISTFILE
      | OpenArchiveFlags::MPQ_OPEN_NO_ATTRIBUTES
      | OpenArchiveFlags::STREAM_FLAG_READ_ONLY,
  )
  .map_err(Into::into)
}

enum Archive<'a> {
  File(FileArchive),
  Memory(MemoryArchive<'a>),
}

impl<'a> Archive<'a> {
  fn read_file_all(&mut self, path: &str) -> Result<Vec<u8>> {
    let bytes = match *self {
      Archive::File(ref mut archive) => archive.open_file(path)?.read_all()?,
      Archive::Memory(ref mut archive) => archive.read_file(path)?,
    };
    Ok(bytes)
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
    let _data = map.render_preview_png();
    // std::fs::write(format!("{}.png", name), data).unwrap()
  }
}

#[test]
fn test_open_storage() {
  let storage = W3Storage::from_env().unwrap();
  let _map = W3Map::open_storage(&storage, "maps\\(4)adrenaline.w3m").unwrap();
  std::fs::write("adrenaline.png", _map.render_preview_png()).unwrap()
}
