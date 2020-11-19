use winapi::shared::minwindef::*;
use winapi::um::winnt::*;

#[allow(unused)]
extern "C" {
  pub fn get_last_error() -> u32;
  pub fn get_version(file_name: LPCWSTR, out: *mut DWORD) -> bool;
  pub fn get_process_path_by_window_title(
    title: LPCWSTR,
    buffer: *mut u16,
    buffer_len: u32,
  ) -> GetProcessPathByWindowTitleResult;
}

#[allow(unused)]
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GetProcessPathByWindowTitleResult {
  Ok = 0,
  WindowNotFound = 1,
  GetWindowThreadProcessId = 2,
  OpenProcess = 3,
  GetModuleFileNameExW = 4,
}

#[test]
fn test_get_version() {
  use std::os::windows::ffi::OsStrExt;
  use std::path::PathBuf;
  let mut path: Vec<u16> =
    PathBuf::from(r#"C:\Program Files (x86)\Warcraft III\_retail_\x86_64\Warcraft III.exe"#)
      .as_os_str()
      .encode_wide()
      .collect();
  path.push(0);
  let mut out: [u32; 4] = [0; 4];
  let v = unsafe { get_version(path.as_ptr() as *const _, out.as_mut_ptr()) };
  dbg!(v, out);
}

#[test]
fn test_get_process_path_by_window_title() {
  use std::ffi::OsString;
  use std::os::windows::ffi::OsStrExt;
  use widestring::U16CString;
  let mut out = Vec::<u16>::with_capacity(256);
  out.resize(256, 0);
  let title = OsString::from("Warcraft III".to_string());
  let mut title: Vec<u16> = title.encode_wide().collect();
  title.push(0);
  let r =
    unsafe { get_process_path_by_window_title(title.as_ptr() as *const _, out.as_mut_ptr(), 256) };
  dbg!(r);

  let path = unsafe { U16CString::from_ptr_str(out.as_ptr()) }.to_os_string();

  dbg!(std::path::PathBuf::from(path));
}
