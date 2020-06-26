//! BLIzzard Picture image format decoder.
//!
//! Author:  Niels A.D.
//! Project: gowarcraft3 (https://github.com/nielsAD/gowarcraft3)
//! License: Mozilla Public License, v2.0
//!
//! Ported from https://github.com/nielsAD/gowarcraft3/blob/master/file/blp/blp.go

use flo_util::binary::*;
use flo_util::BinDecode;
use image::{DynamicImage, GenericImageView, ImageFormat};

const COMPRESSION_JPEG: u32 = 0;
const SOI_HEADER: &[u8] = &[
  0xFF, 0xD8, 0xFF, 0xEE, 0x00, 0x0E, //App14Marker
  b'A', b'd', b'o', b'b', b'e', 0, 0, 0, 0, 0, 0, 0,
];

pub struct BLPImage {
  image: DynamicImage,
}

impl std::fmt::Debug for BLPImage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "BLPImage(witdh = {}, height = {})",
      self.image.width(),
      self.image.height()
    )
  }
}

impl BLPImage {
  pub fn to_bytes(&self) -> Vec<u8> {
    self.image.to_bytes()
  }
}

impl BinDecode for BLPImage {
  const MIN_SIZE: usize = BLP1Header::MIN_SIZE;
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    let size = buf.remaining();
    let header = BLP1Header::decode(buf)?;
    match header.alpha_bits {
      0 | 8 => {}
      v => return Err(BinDecodeError::failure(format!("invalid alpha bit: {}", v))),
    }
    if header.compression != COMPRESSION_JPEG {
      return Err(BinDecodeError::failure(format!(
        "unsupported compression type: {}",
        header.compression
      )));
    }

    let mipmap_offset_original = header.mipmap_offsets[0] as usize;
    let mipmap_len = header.mipmap_lengths[0] as usize;

    if mipmap_offset_original == 0 || mipmap_len == 0 {
      return Err(BinDecodeError::failure("invalid mipmap data"));
    }

    buf.check_size(4)?;
    let image_buf_size = u32::decode(buf)? as usize;

    buf.check_size(image_buf_size as usize)?;
    let mut img_buf = BytesMut::with_capacity((image_buf_size + mipmap_len) as usize);
    img_buf.resize(img_buf.capacity(), 0);
    buf.copy_to_slice(&mut img_buf[..image_buf_size]);

    let offset = mipmap_offset_original
      .checked_sub(size - buf.remaining())
      .ok_or_else(|| BinDecodeError::failure("invalid mipmap offset"))?;

    buf.check_size(offset + mipmap_len)?;
    buf.advance(offset);
    buf.copy_to_slice(&mut img_buf[image_buf_size..]);

    let image = image::load_from_memory_with_format(&img_buf, ImageFormat::Jpeg)
      .or_else(|e| {
        if e.to_string().contains("Adobe APP14") {
          let mut patched = Vec::with_capacity(image_buf_size - 2 + SOI_HEADER.len());
          patched.extend(SOI_HEADER);
          patched.extend(&img_buf[2..]);
          image::load_from_memory_with_format(&patched, ImageFormat::Jpeg)
        } else {
          Err(e)
        }
      })
      .map_err(|e| BinDecodeError::failure(format!("decode jpeg: {:?}", e)))?;

    Ok(Self { image })
  }
}

#[derive(Debug, BinDecode)]
struct BLP1Header {
  #[bin(eq = & b"BLP1")]
  _magic: [u8; 4],
  compression: u32,
  alpha_bits: u32,
  width: u32,
  height: u32,
  flags: u32,
  has_mipmap: u32,
  mipmap_offsets: [u32; 16],
  mipmap_lengths: [u32; 16],
}

#[test]
fn test_blp_to_jpg() {
  let buf = std::fs::read("../../deps/wc3-samples/map/war3mapMap.blp").unwrap();
  dbg!(BLPImage::decode(&mut buf.as_slice()).unwrap());
}
