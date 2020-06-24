pub mod binary;
pub mod dword_string;
pub mod error;
pub mod stat_string;

pub use dhost_codegen::*;

pub fn dump_hex<T>(data: T)
where
  T: AsRef<[u8]>,
{
  use pretty_hex::*;
  println!("{:?}", data.as_ref().hex_dump());
}
