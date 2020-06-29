use flo_util::BinDecode;

#[derive(Debug, BinDecode)]
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
  pub rgba: [u8; 4],
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

#[test]
fn test_parse_minimap_icons() {
  use flo_util::binary::BinDecode;
  let mut map = crate::open_archive(flo_util::sample_path!("map", "(2)ConcealedHill.w3x")).unwrap();

  let bytes = map.open_file("war3map.mmp").unwrap().read_all().unwrap();

  let icons = MinimapIcons::decode(&mut bytes.as_slice()).unwrap();
  dbg!(&icons);
  assert_eq!(icons.len(), 15)
}
