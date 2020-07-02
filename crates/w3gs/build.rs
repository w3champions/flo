fn main() {
  prost_build::compile_protos(&["src/protocol/w3gs.proto"], &["src/"]).unwrap();
}
