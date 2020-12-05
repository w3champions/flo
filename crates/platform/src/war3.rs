#[cfg(windows)]
mod windows {
  use std::os::windows::ffi::OsStrExt;
  use std::path::{Path, PathBuf};

  use crate::error::{Error, Result};
  use crate::windows_bindings::GetProcessPathByWindowTitleResult;

  pub fn get_war3_version(path: &Path) -> Result<String> {
    let mut path_wstr: Vec<u16> = path.as_os_str().encode_wide().collect();
    path_wstr.push(0);
    let mut out: [u32; 4] = [0; 4];

    let ok = unsafe { crate::windows_bindings::get_version(path_wstr.as_ptr(), out.as_mut_ptr()) };
    if !ok {
      return Err(Error::GetWar3Version);
    }
    Ok(format!("{}.{}.{}.{}", out[0], out[1], out[2], out[3]))
  }

  pub fn get_running_war3_executable_path() -> Result<Option<PathBuf>> {
    use std::ffi::OsString;
    use widestring::U16CString;
    let mut out = Vec::<u16>::with_capacity(256);
    out.resize(256, 0);
    let title_osstr = OsString::from("Warcraft III".to_string());
    let mut title: Vec<u16> = title_osstr.encode_wide().collect();
    title.push(0);
    let r = unsafe {
      crate::windows_bindings::get_process_path_by_window_title(
        title.as_ptr() as *const _,
        out.as_mut_ptr(),
        256,
      )
    };

    if r != GetProcessPathByWindowTitleResult::Ok {
      if r == GetProcessPathByWindowTitleResult::WindowNotFound {
        return Ok(None);
      } else {
        return Err(Error::GetRunningWar3Path(r as u32));
      }
    }

    let path = unsafe { U16CString::from_ptr_str(out.as_ptr()) }.to_os_string();

    let path = std::path::PathBuf::from(path);

    if path.file_stem() != Some(&title_osstr) {
      return Ok(None);
    }

    Ok(Some(path))
  }
}
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos {
  use crate::error::Result;
  use serde::Deserialize;
  use std::path::Path;

  pub fn get_war3_version(path: &Path) -> Result<String> {
    #[derive(Deserialize)]
    struct Plist {
      #[serde(rename = "CFBundleVersion")]
      cf_bundle_version: String,
    }

    let value: Plist = plist::from_file(path.join("Contents/Info.plist"))?;

    Ok(value.cf_bundle_version)
  }

  #[test]
  fn get_mac_war3_version() {
    use crate::path::detect_installation_path;
    let path = detect_installation_path()
      .unwrap()
      .join("_retail_/x86_64/Warcraft III.app");
    dbg!(&path);
    let v = get_war3_version(&path).unwrap();
    dbg!(v);
  }
}
#[cfg(target_os = "macos")]
pub use self::macos::*;
