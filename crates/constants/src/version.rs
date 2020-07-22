use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct Version {
  pub major: u8,
  pub minor: u8,
  pub patch: u8,
}

impl Version {
  pub fn parse(v: &'static str) -> Self {
    let parts: Vec<u8> = v.split('.').map(|v| v.parse::<u8>().unwrap()).collect();
    assert_eq!(parts.len(), 3);
    Version {
      major: parts[0],
      minor: parts[1],
      patch: parts[2],
    }
  }
}

impl fmt::Display for Version {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
  }
}
