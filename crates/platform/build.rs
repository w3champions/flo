#[cfg(windows)]
fn main() {
  cc::Build::new()
    .file("src/windows_bindings.cpp")
    .compile("windows_bindings");
  println!("cargo:rustc-link-lib=Version");
  println!("cargo:rustc-link-lib=User32");
}

#[cfg(not(windows))]
fn main() {}
