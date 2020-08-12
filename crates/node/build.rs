use std::fs;
use std::path::Path;

fn main() {
  let out_dir = std::env::var_os("OUT_DIR").unwrap();
  let pkg_version = env!("CARGO_PKG_VERSION");
  let version = flo_constants::version::Version::parse(pkg_version);
  let version_path = Path::new(&out_dir).join("flo_node_version.rs");
  fs::write(
    version_path,
    format!(
      r#"pub const FLO_NODE_VERSION: flo_constants::version::Version = flo_constants::version::Version {{
      major: {major},
      minor: {minor},
      patch: {patch},
    }};
      pub const FLO_NODE_VERSION_STRING: &str = "{version_str}";
    "#,
      major = version.major,
      minor = version.minor,
      patch = version.patch,
      version_str = pkg_version
    ),
  )
    .unwrap()
}
