#[cfg(windows)]
fn main() {
  use std::env::var;
  use std::fs;
  use std::path::Path;

  let src = include_str!("./resource.rc");
  let path = Path::new(&var("OUT_DIR").unwrap()).join("resource.rc");
  let version = var("CARGO_PKG_VERSION").unwrap();

  let res = format!(
    r##"{src}
1 VERSIONINFO
FILEVERSION {version2},0
PRODUCTVERSION {version2},0
{{
  BLOCK "StringFileInfo"
  {{
    BLOCK "040904b0"
    {{
      VALUE "CompanyName", "Ke Xu"
      VALUE "FileDescription", "Flo Worker"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "flo-worker"
      VALUE "LegalCopyright", "Flo 2020-2021. All rights reserved."
      VALUE "ProductName", "Flo Worker"
      VALUE "OriginalFilename", "flo-worker.exe"
      VALUE "ProductVersion", "{version}"
      VALUE "CompanyShortName", "Ke Xu"
      VALUE "ProductShortName", "Flo Worker"
    }}
  }}
  BLOCK "VarFileInfo"
  {{
      VALUE "Translation", 0x409, 1200
  }}
}}
    "##,
    src = src,
    version = version,
    version2 = version.replace(".", ",")
  );

  fs::write(&path, &res).unwrap();

  embed_resource::compile(path);
}

#[cfg(not(windows))]
fn main() {}
