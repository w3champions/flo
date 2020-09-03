use flo_controller::{serve_grpc, serve_socket, ControllerStateRef};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log_subscriber::init_env_override(
      "flo_controller_service=debug,flo_controller=debug,flo_event=debug",
    );
  }
  #[cfg(not(debug_assertions))]
  flo_log_subscriber::init();

  let state = ControllerStateRef::init().await?;

  #[cfg(unix)]
  {
    use tokio::signal::unix::{signal, SignalKind};
    let mut stream = signal(SignalKind::hangup())?;
    tokio::spawn({
      let state = state.clone();
      async move {
        loop {
          stream.recv().await;
          tracing::info!("reloading");
          if let Err(err) = state.reload().await {
            tracing::error!("reload error: {}", err);
          }
        }
      }
    });
  }

  tokio::try_join!(serve_grpc(state.clone()), serve_socket(state.clone()))?;

  Ok(())
}
