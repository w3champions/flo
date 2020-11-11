use std::path::PathBuf;

pub fn detect_user_data_path() -> Option<PathBuf> {
  // let mut path = PathBuf::from("")?;
  // path.push("Warcraft III");
  // if std::fs::metadata(&path).is_ok() {
  //   Some(path)
  // } else {
  //   None
  // }
  unimplemented!()
}

pub fn detect_installation_path() -> Option<PathBuf> {
  // let try_list = vec![
  //   {
  //     let mut path = get_known_folder_path(winapi::um::knownfolders::FOLDERID_ProgramFilesX86)?;
  //     path.push("Warcraft III");
  //     path
  //   },
  //   {
  //     let mut path = get_known_folder_path(winapi::um::knownfolders::FOLDERID_ProgramFilesX64)?;
  //     path.push("Warcraft III");
  //     path
  //   },
  // ];
  //
  // for path in try_list {
  //   let full = path.join("Warcraft III Launcher.exe");
  //   if std::fs::metadata(full).is_ok() {
  //     return Some(path);
  //   }
  // }
  //
  // None
  unimplemented!()
}

#[test]
fn test_macos() {
  assert!(dbg!(detect_user_data_path()).is_some());
  assert!(dbg!(detect_installation_path()).is_some());
}
