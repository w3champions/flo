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
macro_rules! sample_bytes {
  (
    $($rpath:expr),+
  ) => {
    std::fs::read(
    std::path::Path::new(&[
      "..", "..", "deps", "wc3-samples", $($rpath),*
    ].iter().collect::<std::path::PathBuf>())).unwrap()
  };
}
