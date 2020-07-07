use crate::{BinDecode, BinEncode};
use std::fmt;

#[derive(Clone, Copy, BinEncode, BinDecode)]
#[bin(mod_path = "crate::binary")]
pub struct DwordString {
  bytes: [u8; 4],
}

impl DwordString {
  pub fn new(bstr: &[u8; 4]) -> Self {
    DwordString {
      bytes: [bstr[3], bstr[2], bstr[1], bstr[0]],
    }
  }

  pub fn as_bytes(&self) -> &[u8; 4] {
    &self.bytes
  }

  pub fn from_bytes(bytes: [u8; 4]) -> Self {
    DwordString { bytes }
  }

  pub fn to_string(&self) -> String {
    self
      .bytes
      .iter()
      .rev()
      .cloned()
      .filter_map(|byte| {
        if byte != 0 {
          Some(char::from(byte))
        } else {
          None
        }
      })
      .collect()
  }
}

impl fmt::Display for DwordString {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "'{}'", self.to_string())
  }
}

impl fmt::Debug for DwordString {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "'{}'", self.to_string())
  }
}

impl<'a> PartialEq<&'a [u8; 4]> for DwordString {
  fn eq(&self, other: &&'a [u8; 4]) -> bool {
    self.bytes[0] == other[3]
      && self.bytes[1] == other[2]
      && self.bytes[2] == other[1]
      && self.bytes[3] == other[0]
  }
}

#[test]
fn test_dword_string() {
  assert_eq!(DwordString::new(b"W3XP").as_bytes(), &[80_u8, 88, 51, 87]);
  assert_eq!(
    DwordString::from_bytes([80_u8, 88, 51, 87]).to_string(),
    "W3XP"
  );
  assert_eq!(DwordString::new(b"W3XP"), b"W3XP");
}
