use home_dir::HomeDirExt;
use std::path::PathBuf;

pub fn detect_user_data_path(ptr: bool) -> Option<PathBuf> {
  let path = PathBuf::from(
    if ptr {"~/Library/Application Support/Blizzard/Warcraft III Public Test"}
      else {"~/Library/Application Support/Blizzard/Warcraft III"})
    .expand_home()
    .ok()?;
  if std::fs::metadata(&path).is_ok() {
    Some(path)
  } else {
    None
  }
}

pub fn detect_installation_path() -> Option<PathBuf> {
  let path = PathBuf::from("/Applications/Warcraft III");
  if std::fs::metadata(path.join("Warcraft III Launcher.app")).is_ok() {
    return Some(path);
  }
  None
}

#[test]
fn test_macos() {
  assert!(dbg!(detect_user_data_path()).is_some());
  assert!(dbg!(detect_installation_path()).is_some());
}
