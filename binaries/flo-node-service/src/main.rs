use flo_node::serve;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(windows)]
  unsafe {
    winapi::um::timeapi::timeBeginPeriod(1);
  }

  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log_subscriber::init_env_override("flo_node_service=info,flo_node=info");
  }
  #[cfg(not(debug_assertions))]
  {
    flo_log_subscriber::init();
  }

  tracing::info!("starting.");

  serve().await?;

  Ok(())
}
