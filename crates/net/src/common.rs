use std::fmt::{Display, Formatter, Result};

use crate::proto::flo_common::Version;

impl Display for Version {
  fn fmt(&self, f: &mut Formatter<'_>) -> Result {
    write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
  }
}
