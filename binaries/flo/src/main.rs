#[tokio::main]
async fn main() {
  flo_log_subscriber::init_env_override("flo=debug,flo_lan=debug");

  let task = flo_client::start(Default::default()).await.unwrap();
  let join = tokio::spawn(task.serve());
  let ctrl_c = tokio::signal::ctrl_c();

  tokio::select! {
    res = join => res.unwrap(),
    _ = ctrl_c => {},
  }
}
