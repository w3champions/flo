use flo_node::{serve_echo, serve_node};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log::init_env("flo_node_service=debug,flo_node=debug");
  }

  tokio::try_join!(serve_echo(), serve_node())?;

  Ok(())
}
