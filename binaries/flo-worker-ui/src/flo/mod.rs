use super::Opt;

use anyhow::Result;
use flo_client::StartConfig;
use tokio::runtime::Runtime;

pub fn run_flo(opt: Opt) -> (bool, String) {
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
    Ok((port, mut _rt)) => {
      tracing::info!("running on port: {}", port);
      // it's inside GUI, no need to block on it I guess
      // rt.block_on(tokio::signal::ctrl_c()).unwrap();
      (true, port.to_string())
    }
    Err(err) => {
      tracing::error!("failed to run flo clinet: {:?}", err);
      (false, err.to_string())
    }
  }
}

#[allow(dead_code)]
pub async fn perform_run_flo(opt: Opt) -> (bool, String) {
  run_flo(opt)
}
