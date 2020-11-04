#[tokio::main]
async fn main() {
  flo_log_subscriber::init_env_override("flo=debug,flo_lan=debug");

  tokio::select! {
    res = flo_client::start() => res.unwrap().await,
    _ = tokio::signal::ctrl_c() => {},
  }
}
