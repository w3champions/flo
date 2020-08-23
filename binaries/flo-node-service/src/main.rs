use flo_node::{serve_client, serve_controller, serve_echo, serve_metrics};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log_subscriber::init_env("flo_node_service=debug,flo_node=debug");
  }

  tokio::try_join!(
    serve_client(),
    serve_controller(),
    serve_echo(),
    serve_metrics()
  )?;

  Ok(())
}
