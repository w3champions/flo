fn main() {
  prost_build::compile_protos(&["src/wc3.proto"], &["src/"]).unwrap();
  #[cfg(target_os = "windows")]
  {
    let dir =
      std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("env CARGO_MANIFEST_DIR"))
        .join("../../deps/bonjour-sdk-windows");
    println!(
      "cargo:rustc-link-search=native={}",
      dir.join("Lib/x64").display()
    );
  }
}
