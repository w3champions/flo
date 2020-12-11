use super::Opt;

use anyhow::Result;
use flo_client::StartConfig;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;

use once_cell::sync::OnceCell;
use once_cell::sync::Lazy;

static RT: Lazy<Mutex<Runtime>> = Lazy::new(|| { Mutex::new(Runtime::new().unwrap()) });

async fn run_flo(opt: Opt) -> Result<u16> {
  let mut rt = RT.lock().await;
  let client = rt.block_on(flo_client::start(StartConfig {
    token: opt.token,
    installation_path: opt.installation_path,
    controller_host: opt.controller_host.clone(),
    ..Default::default()
  }))?;
  let port = client.port();
  rt.spawn(
    client.serve()
  );
  Ok(port)
}

pub async fn perform_run_flo(opt: Opt) -> (bool, String) {
  let mut res = run_flo(opt).await
    .map_err(|err| anyhow::format_err!("Start flo worker failed: {:?}", err));

  match res {
    Ok(port) => {
      tracing::info!("running on port: {}", port);
      (true, port.to_string())
    }
    Err(err) => {
      tracing::error!("failed to run flo clinet: {:?}", err);
      (false, err.to_string())
    }
  }
}
