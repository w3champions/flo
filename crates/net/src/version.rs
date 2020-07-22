impl From<flo_constants::version::Version> for crate::proto::flo_common::Version {
  fn from(v: flo_constants::version::Version) -> Self {
    crate::proto::flo_common::Version {
      major: v.major as i32,
      minor: v.minor as i32,
      patch: v.patch as i32,
    }
  }
}
