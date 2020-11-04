use std::fmt;

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq)]
pub struct Version {
  pub major: i32,
  pub minor: i32,
  pub patch: i32,
}

impl Version {
  pub fn parse(v: &'static str) -> Self {
    let parts: Vec<i32> = v.split('.').map(|v| v.parse::<i32>().unwrap()).collect();
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
