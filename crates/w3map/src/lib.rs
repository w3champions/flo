use flo_util::binary::BinDecode;
use std::path::Path;
use stormlib::{Archive, OpenArchiveFlags};

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
    let mut archive = open_archive(path)?;

    Ok(W3Map {
      info: {
        let bytes = archive.open_file("war3map.w3i")?.read_all()?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadInfo)?
      },
      image: {
        let bytes = archive.open_file("war3mapMap.blp")?.read_all()?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadImage)?
      },
      minimap_icons: {
        let bytes = archive.open_file("war3map.mmp")?.read_all()?;
        BinDecode::decode(&mut bytes.as_slice()).map_err(Error::ReadMinimapIcons)?
      },
      trigger_strings: {
        let bytes = archive.open_file("war3map.wts")?.read_all()?;
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

pub(crate) fn open_archive<P: AsRef<Path>>(path: P) -> Result<Archive> {
  Archive::open(
    path,
    OpenArchiveFlags::MPQ_OPEN_NO_LISTFILE
      | OpenArchiveFlags::MPQ_OPEN_NO_ATTRIBUTES
      | OpenArchiveFlags::STREAM_FLAG_READ_ONLY,
  )
  .map_err(Into::into)
}

#[test]
fn test_open_map() {
  for name in &[
    "(2)ConcealedHill.w3x",
    "(8)Sanctuary_LV.w3x",
    "test_roc.w3m",
    "test_tft.w3x",
  ] {
    let map = W3Map::open(flo_util::sample_path!("map", name)).unwrap();
    let _data = map.render_preview_png();
    // std::fs::write(format!("{}.png", name), data).unwrap()
  }
}
