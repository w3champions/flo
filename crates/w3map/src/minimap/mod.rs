use flo_util::BinDecode;
use image::{GenericImage, GenericImageView};
use image::{ImageBuffer, Rgba};
use lazy_static::lazy_static;

#[derive(Debug, BinDecode, Default)]
pub struct MinimapIcons {
  #[bin(eq = 0)]
  version: i32,
  num: u32,
  #[bin(repeat = "num")]
  icons: Vec<MinimapIcon>,
}

impl MinimapIcons {
  pub fn len(&self) -> usize {
    self.icons.len()
  }

  pub fn iter(&self) -> impl Iterator<Item = &MinimapIcon> {
    self.icons.iter()
  }

  pub fn as_slice(&self) -> &[MinimapIcon] {
    self.icons.as_slice()
  }
}

#[derive(Debug, BinDecode)]
pub struct MinimapIcon {
  pub type_: MinimapIconType,
  pub pos_x: u32,
  pub pos_y: u32,
  pub bgra: [u8; 4],
}

impl MinimapIcon {
  pub(crate) fn draw_into<I: GenericImage>(&self, image: &mut I)
  where
    I: GenericImage<Pixel = Rgba<u8>>,
  {
    use image::imageops::{overlay, overlay_bounds};

    if let MinimapIconType::UnknownValue(_) = self.type_ {
      return;
    }

    let icon = self.type_.image_buffer();
    let draw_x = self.pos_x.saturating_sub(8);
    let draw_y = self.pos_y.saturating_sub(8);
    let (view_w, view_h) = overlay_bounds(image.dimensions(), icon.dimensions(), draw_x, draw_y);

    if view_w == 0 || view_h == 0 {
      return;
    }

    let view = icon.view(0, 0, view_w, view_h);

    if self.bgra == [255, 255, 255, 255] {
      overlay(image, &view, draw_x, draw_y);
    } else {
      if self.bgra[3] != 0 {
        overlay(
          image,
          &image::ImageBuffer::from_fn(view_w, view_h, |x, y| {
            let pixel = view.get_pixel(x, y);
            let [r, g, b, a] = pixel.0;
            if pixel.0[3] != 0 {
              return image::Rgba([
                ((r as f32) * (self.bgra[2] as f32 / 255_f32)) as u8,
                ((g as f32) * (self.bgra[1] as f32 / 255_f32)) as u8,
                ((b as f32) * (self.bgra[0] as f32 / 255_f32)) as u8,
                ((a as f32) * (self.bgra[3] as f32 / 255_f32)) as u8,
              ]);
            }
            pixel
          }),
          draw_x,
          draw_y,
        )
      }
    }
  }
}

#[derive(Debug, BinDecode)]
#[bin(enum_repr(u32))]
pub enum MinimapIconType {
  #[bin(value = 0)]
  Gold,
  #[bin(value = 1)]
  Neutral,
  #[bin(value = 2)]
  Cross,
  UnknownValue(u32),
}

impl MinimapIconType {
  fn image_buffer(&self) -> &'static ImageBuffer<Rgba<u8>, &'static [u8]> {
    lazy_static! {
      static ref CROSS: ImageBuffer<Rgba<u8>, &'static [u8]> =
        ImageBuffer::from_raw(16, 16, include_bytes!("./icons/cross.rgba") as &[u8]).unwrap();
      static ref GOLD: ImageBuffer<Rgba<u8>, &'static [u8]> =
        ImageBuffer::from_raw(16, 16, include_bytes!("./icons/gold.rgba") as &[u8]).unwrap();
      static ref NEUTRAL: ImageBuffer<Rgba<u8>, &'static [u8]> =
        ImageBuffer::from_raw(16, 16, include_bytes!("./icons/neutral.rgba") as &[u8]).unwrap();
      static ref UNKNOWN: ImageBuffer<Rgba<u8>, &'static [u8]> =
        ImageBuffer::from_raw(16, 16, &[0_u8; 16 * 16 * 4] as &[u8]).unwrap();
    }
    match *self {
      MinimapIconType::Gold => &GOLD,
      MinimapIconType::Neutral => &NEUTRAL,
      MinimapIconType::Cross => &CROSS,
      MinimapIconType::UnknownValue(_) => &UNKNOWN,
    }
  }
}

#[test]
fn test_parse_minimap_icons() {
  use flo_util::binary::BinDecode;
  let mut map = crate::open_archive(flo_util::sample_path!("map", "(2)ConcealedHill.w3x")).unwrap();

  let bytes = map.open_file("war3map.mmp").unwrap().read_all().unwrap();

  let icons = MinimapIcons::decode(&mut bytes.as_slice()).unwrap();
  dbg!(&icons);
  assert_eq!(icons.len(), 15)
}

#[test]
fn test_prepare_icon() {
  use image::ImageFormat;
  let cross =
    image::load_from_memory_with_format(include_bytes!("./icons/neutral.png"), ImageFormat::Png)
      .unwrap();
  let pixels: Vec<_> = cross
    .pixels()
    .flat_map(|(_, _, Rgba([r, g, b, a]))| vec![r, g, b, a].into_iter())
    .collect();
  std::fs::write("src/minimap/icons/neutral.rgba", pixels).unwrap()
}
