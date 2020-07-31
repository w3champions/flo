fn main() {
  let mut prost_build = prost_build::Config::new();
  prost_build.type_attribute(".", "#[derive(Serialize, Deserialize)]");
  prost_build
    .compile_protos(&["src/proto/connect.proto"], &["src"])
    .unwrap();
}
