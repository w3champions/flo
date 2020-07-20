fn main() {
  prost_build::compile_protos(&["src/proto/connect.proto"], &["src"]).unwrap();
}
