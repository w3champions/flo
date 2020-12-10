mod log;

use anyhow::Result;
use flo_client::StartConfig;
use std::io::Write;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::runtime::Runtime;

#[derive(Debug, StructOpt)]
#[structopt(name = "flo-worker", about = "Flo worker process.")]
struct Opt {
  #[structopt(long)]
  debug: bool,

  #[structopt(long)]
  token: Option<String>,

  #[structopt(long, parse(from_os_str))]
  installation_path: Option<PathBuf>,

  #[structopt(long)]
  controller_host: Option<String>,
}

fn main() {
  #[cfg(windows)]
  unsafe {
    winapi::um::processthreadsapi::SetPriorityClass(
      winapi::um::processthreadsapi::GetCurrentProcess(),
      winapi::um::winbase::ABOVE_NORMAL_PRIORITY_CLASS,
    );
  }

  let opt = Opt::from_args();

  log::init(opt.debug);

  let res = std::panic::catch_unwind(|| -> Result<_> {
    let mut rt = Runtime::new()?;
    let client = rt.block_on(flo_client::start(StartConfig {
      token: opt.token,
      installation_path: opt.installation_path,
      controller_host: opt.controller_host.clone(),
      ..Default::default()
    }))?;
    let port = client.port();
    rt.spawn(client.serve());
    Ok((port, rt))
  })
  .map_err(|err| anyhow::format_err!("Start flo worker failed: {:?}", err))
  .and_then(std::convert::identity);

  match res {
    Ok((port, mut rt)) => {
      let msg = serde_json::to_string(&serde_json::json!({
        "version": flo_client::FLO_VERSION.to_string(),
        "port": port
      }))
      .unwrap();
      let mut stdout = std::io::stdout();
      stdout.write(msg.as_bytes()).unwrap();
      stdout.flush().unwrap();
      rt.block_on(tokio::signal::ctrl_c()).unwrap();
    }
    Err(err) => {
      let msg = serde_json::to_string(&serde_json::json!({
        "error": err.to_string()
      }))
      .unwrap();
      std::io::stderr().write(msg.as_bytes()).unwrap();
    }
  }
}
