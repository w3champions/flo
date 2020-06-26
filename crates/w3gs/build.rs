fn main() {
  prost_build::compile_protos(&["src/w3gs.proto"], &["src/"]).unwrap();
}
