use flo_controller::{serve_grpc, serve_socket, ControllerStateRef};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenv::dotenv()?;

  #[cfg(debug_assertions)]
  flo_log_subscriber::init_env("flo_controller_service=debug,flo_controller=debug,flo_event=debug");

  let state = ControllerStateRef::init().await?;

  #[cfg(not(debug_assertions))]
  {
    state
      .db
      .exec(|conn| flo_controller::migration::run(conn))
      .await?;
  }

  tokio::try_join!(serve_grpc(state.clone()), serve_socket(state.clone()))?;

  Ok(())
}
