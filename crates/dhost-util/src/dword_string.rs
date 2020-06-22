use std::fmt;

pub struct DwordString(u32);

impl DwordString {
  pub fn new(string_value: &'static str) -> Self {
    let bytes = string_value.as_bytes();
    assert!(bytes.len() <= 4);

    let u32_value = bytes
      .into_iter()
      .enumerate()
      .fold(0, |v, (i, byte)| v | (*byte as u32) << (i * 8));

    DwordString(u32_value)
  }

  pub fn as_u32(&self) -> u32 {
    self.0
  }

  pub fn from_u32(v: u32) -> Self {
    DwordString(v)
  }

  pub fn to_string(&self) -> String {
    self
      .0
      .to_le_bytes()
      .iter()
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

#[test]
fn test_dword_string() {
  assert_eq!(DwordString::new("W3XP").as_u32(), 0x50583357);
  assert_eq!(DwordString::from_u32(0x50583357).to_string(), "W3XP");
}
