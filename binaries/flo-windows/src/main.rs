#![windows_subsystem = "windows"]

mod client;
mod log;

extern crate native_windows_derive as nwd;
extern crate native_windows_gui as nwg;

use nwd::NwgUi;
use nwg::NativeUi;
use std::cell::RefCell;
use std::ffi::CString;
use std::ptr;
use std::thread;
use winapi::um::shellapi::ShellExecuteA;

#[derive(Default, NwgUi)]
pub struct App {
  #[nwg_control(size: (300, 300), title: "Flo", flags: "WINDOW|VISIBLE")]
  #[nwg_events( OnWindowClose: [App::on_close(SELF, EVT_DATA)], OnInit: [App::init] )]
  window: nwg::Window,

  #[nwg_resource]
  embed: nwg::EmbedResource,

  #[nwg_layout(parent: window, spacing: 1)]
  grid: nwg::GridLayout,

  #[nwg_control(text: "Starting...", h_align: nwg::HTextAlign::Center)]
  #[nwg_layout_item(layout: grid, row: 0, col: 0)]
  status_label: nwg::Label,

  #[nwg_control(text: "Open UI")]
  #[nwg_layout_item(layout: grid, col: 0, row: 1)]
  #[nwg_events( OnButtonClick: [App::open_ui] )]
  ui_button: nwg::Button,

  #[nwg_control(text: "Logs")]
  #[nwg_layout_item(layout: grid, col: 0, row: 2)]
  #[nwg_events( OnButtonClick: [App::open_logs] )]
  logs_button: nwg::Button,

  #[nwg_control]
  #[nwg_events( OnNotice: [App::read_flo_start_result] )]
  start_notice: nwg::Notice,
  start_result: RefCell<Option<ClientState>>,
}

enum ClientState {
  Pending(thread::JoinHandle<client::Result<client::Runtime>>),
  Started(client::Runtime),
}

impl App {
  fn init(&self) {
    let em = &self.embed;

    let icon = em.icon_str("MAINICON", None).unwrap();

    self.window.set_icon(Some(&icon));
    self.window.set_text("W3Champions Test Client");

    let sender = self.start_notice.sender();
    *self.start_result.borrow_mut() = Some(ClientState::Pending(thread::spawn(move || {
      let res = client::init();
      sender.notice();
      res
    })));
  }

  fn open_ui(&self) {
    #[cfg(debug_assertions)]
    let cstr = CString::new("http://localhost:3000").unwrap();
    #[cfg(not(debug_assertions))]
    let cstr = CString::new("https://w3flo.com").unwrap();
    unsafe {
      ShellExecuteA(
        ptr::null_mut(),
        ptr::null_mut(),
        cstr.as_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        1,
      );
    }
  }

  fn open_logs(&self) {
    let cstr = CString::new("logs").unwrap();
    unsafe {
      ShellExecuteA(
        ptr::null_mut(),
        ptr::null_mut(),
        cstr.as_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        1,
      );
    }
  }

  fn on_close(&self, data: &nwg::EventData) {
    let p = nwg::MessageParams {
      title: "Quit",
      content: "Are you sure? Ongoing game will disconnect.",
      buttons: nwg::MessageButtons::YesNo,
      icons: nwg::MessageIcons::Warning,
    };
    let data = if let nwg::EventData::OnWindowClose(ref data) = data {
      data
    } else {
      return;
    };

    if nwg::modal_message(&self.window, &p) == nwg::MessageChoice::Yes {
      data.close(true);
      nwg::stop_thread_dispatch();
    } else {
      data.close(false);
    }
  }

  fn read_flo_start_result(&self) {
    let mut data = self.start_result.borrow_mut();
    let message = match data.take() {
      Some(ClientState::Pending(handle)) => match handle.join() {
        Ok(res) => match res {
          Ok(rt) => {
            data.replace(ClientState::Started(rt));
            format!("v{}", flo_client::FLO_VERSION)
          }
          Err(err) => {
            nwg::modal_error_message(&self.window, "W3Champions", &err.to_string());
            nwg::stop_thread_dispatch();
            return;
          }
        },
        Err(err) => {
          let msg = format!(
            "Crashed: {}",
            match err.downcast_ref::<String>() {
              None => format!("Unknown Error"),
              Some(display) => display.to_string(),
            }
          );
          nwg::modal_error_message(&self.window, "W3Champions", &msg);
          nwg::stop_thread_dispatch();
          return;
        }
      },
      _ => unreachable!(),
    };
    self.status_label.set_text(&message);
  }
}

fn main() {
  unsafe {
    winapi::um::processthreadsapi::SetPriorityClass(
      winapi::um::processthreadsapi::GetCurrentProcess(),
      winapi::um::winbase::HIGH_PRIORITY_CLASS,
    );
  }

  log::init();

  tracing::info!("start");

  nwg::init().expect("Failed to init Native Windows GUI");
  // nwg::Font::set_global_family("Segoe UI").expect("Failed to set default font");

  let mut font = nwg::Font::default();

  nwg::Font::builder()
    // .size(16)
    .family("Segoe UI")
    .build(&mut font)
    .unwrap();

  nwg::Font::set_global_default(Some(font));

  let _app = App::build_ui(Default::default()).expect("Failed to build UI");
  nwg::dispatch_thread_events();
}
