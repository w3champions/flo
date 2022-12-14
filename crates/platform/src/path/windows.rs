use std::path::PathBuf;
use winapi::shared::guiddef::GUID;
use winapi::um::winnt::WCHAR;

fn get_known_folder_path(id: GUID) -> Option<PathBuf> {
  use std::ptr;
  use widestring::UCString;
  use winapi::shared::winerror::S_OK;
  use winapi::um::combaseapi::CoTaskMemFree;
  use winapi::um::shlobj::SHGetKnownFolderPath;

  let mut pwstr: *mut WCHAR = ptr::null_mut();

  unsafe {
    if S_OK
      != SHGetKnownFolderPath(
        &id as *const _,
        0,
        ptr::null_mut(),
        &mut pwstr as *mut *mut WCHAR,
      )
    {
      return None;
    }
  };

  if pwstr.is_null() {
    return None;
  }

  let wstring = unsafe { UCString::from_ptr_str(pwstr) };
  let path = PathBuf::from(wstring.to_os_string());

  unsafe {
    CoTaskMemFree(pwstr.cast());
  }

  Some(path)
}

pub fn detect_user_data_path(ptr: bool) -> Option<PathBuf> {
  let mut path = get_known_folder_path(winapi::um::knownfolders::FOLDERID_Documents)?;
  path.push(if ptr {"Warcraft III Public Test"} else {"Warcraft III"});
  if std::fs::metadata(&path).is_ok() {
    Some(path)
  } else {
    None
  }
}

pub fn detect_installation_path() -> Option<PathBuf> {
  let try_list = vec![
    {
      let mut path = get_known_folder_path(winapi::um::knownfolders::FOLDERID_ProgramFilesX86)?;
      path.push("Warcraft III");
      path
    },
    {
      let mut path = get_known_folder_path(winapi::um::knownfolders::FOLDERID_ProgramFilesX64)?;
      path.push("Warcraft III");
      path
    },
  ];

  for path in try_list {
    let full = path.join("Warcraft III Launcher.exe");
    if std::fs::metadata(full).is_ok() {
      return Some(path);
    }
  }

  None
}

#[test]
fn test_windows() {
  assert!(dbg!(detect_user_data_path(false)).is_some());
  assert!(dbg!(detect_installation_path()).is_some());
}
