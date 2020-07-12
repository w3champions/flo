use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use winapi::um::winuser::*;

use flo_w3replay::W3Replay;
use flo_w3storage::W3Storage;

const REPLAY_FILE_PATH: &str =
  r#"C:\Users\fluxx\OneDrive\Documents\Warcraft III\BattleNet\298266\Replays\LastReplay.w3g"#;
const TEMP_REPLAY_FILE_PATH: &str =
  r#"C:\Users\fluxx\OneDrive\Documents\Warcraft III\BattleNet\298266\Replays\TempReplay.w3g"#;
const WAR3_PATH: &str = r#"C:\Program Files (x86)\Warcraft III\_retail_\x86_64\Warcraft III.exe"#;

const OUTPUT_DIR: &str = r#"target\map_info_dump"#;

fn main() {
  let storage = W3Storage::from_env().unwrap();

  let maps = storage.list_storage_files("maps\\*").unwrap();

  for (i, path) in maps.iter().enumerate().skip(201) {
    println!("[{}/{}] processing: {}", i + 1, maps.len(), path);

    let (sha1, checksum) = process_map(path);

    let out = Path::new(OUTPUT_DIR).join(format!("{}.json", to_hex(sha1)));
    fs::write(
      out,
      serde_json::to_string_pretty(&serde_json::json!({
        "path": path,
        "sha1": sha1.to_vec(),
        "checksum": checksum
      }))
      .unwrap(),
    )
    .unwrap();
  }
}

fn process_map(path: &str) -> ([u8; 20], u32) {
  let ignore_not_found = |e: std::io::Error| {
    if e.kind() == ErrorKind::NotFound {
      Ok(())
    } else {
      Err(e)
    }
  };

  fs::remove_file(TEMP_REPLAY_FILE_PATH)
    .or_else(ignore_not_found)
    .expect("remove temp replay");
  fs::remove_file(REPLAY_FILE_PATH)
    .or_else(ignore_not_found)
    .expect("remove last replay");

  let mut proc = Command::new(WAR3_PATH)
    .arg("-launch")
    .arg("-loadfile")
    .arg(path)
    .spawn()
    .expect("war3 failed to start");

  loop {
    println!("waiting...");
    sleep(Duration::from_secs(1));

    if let Err(e) = fs::metadata(TEMP_REPLAY_FILE_PATH) {
      if e.kind() == std::io::ErrorKind::NotFound {
        continue;
      }
      panic!("read temp replay meta: {}", e);
    }

    println!("temp replay found");

    unsafe {
      use std::ffi::CString;
      use std::ptr;

      let title = CString::new("Warcraft III").unwrap();

      let hwnd = FindWindowA(ptr::null(), title.as_ptr());
      if hwnd != ptr::null_mut() {
        println!("found window: {:?}", hwnd);
      } else {
        panic!("find window failed");
      }

      sleep(Duration::from_secs(3));

      // SetForegroundWindow(hwnd);

      send_key(VK_RETURN);

      sleep(Duration::from_secs(3));

      send_key(VK_F10);
      send_key(0x45);
      send_key(0x58);
      send_key(0x58);
    }

    proc.wait().unwrap();
    break;
  }

  let replay = W3Replay::open(REPLAY_FILE_PATH).unwrap();
  for r in replay.into_records() {
    let r = r.unwrap();
    if let flo_w3replay::Record::GameInfo(info) = r {
      return (info.game_settings.map_sha1, info.game_settings.map_xoro);
    }
  }
  unreachable!()
}

fn send_key(key: i32) {
  unsafe {
    {
      let mut input = INPUT {
        type_: INPUT_KEYBOARD,
        u: {
          let mut u = INPUT_u::default();
          let ki = u.ki_mut();
          ki.wVk = key as u16;
          ki.dwFlags = 0;
          u
        },
      };

      let n = SendInput(
        1,
        &mut input as *mut INPUT,
        std::mem::size_of::<INPUT>() as i32,
      );
      if n != 1 {
        panic!("send input failed");
      }

      let mut input = INPUT {
        type_: INPUT_KEYBOARD,
        u: {
          let mut u = INPUT_u::default();
          let ki = u.ki_mut();
          ki.wVk = key as u16;
          ki.dwFlags = KEYEVENTF_KEYUP;
          u
        },
      };

      let n = SendInput(
        1,
        &mut input as *mut INPUT,
        std::mem::size_of::<INPUT>() as i32,
      );
      if n != 1 {
        panic!("send input failed");
      }
    }
  }
}

fn to_hex(sha1: [u8; 20]) -> String {
  sha1.iter().map(|b| format!("{:x}", b)).collect()
}
