use flo_observer_consumer::FloObserver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log_subscriber::init_env_override(
      "flo_observer_service=debug,flo_observer_consumer=debug,flo_observer=debug",
    );
  }

  #[cfg(not(debug_assertions))]
  {
    flo_log_subscriber::init();
  }

  tracing::info!("starting.");

  FloObserver::serve().await?;

  Ok(())
}
