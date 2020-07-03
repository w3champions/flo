pub mod binary;
pub mod dword_string;
pub mod error;
pub mod stat_string;

pub use flo_codegen::*;

pub fn dump_hex<T>(data: T)
where
  T: AsRef<[u8]>,
{
  use pretty_hex::*;
  println!("{:?}", data.as_ref().hex_dump());
}

#[macro_export]
macro_rules! sample_path {
  (
    $($rpath:expr),+
  ) => {{
    use std::path::Path;
    use std::ffi::OsStr;
    let mut manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut path = manifest_dir.ancestors()
      .find(|path| path.file_name() == Some(OsStr::new("crates")))
      .and_then(|path| path.parent())
      .map(|path| path.to_owned())
      .unwrap();
    path.push("deps");
    path.push("wc3-samples");
    $(
      path.push($rpath);
    )*
    path
  }};
}

#[macro_export]
macro_rules! sample_bytes {
  (
    $($rpath:expr),+
  ) => {
    std::fs::read(
      $crate::sample_path!($($rpath),*)
    ).unwrap()
  };
}
