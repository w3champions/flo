fn main() {
  prost_build::compile_protos(&["src/wc3.proto"], &["src/"]).unwrap();
}
