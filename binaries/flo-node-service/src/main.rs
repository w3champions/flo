use flo_node::serve;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  #[cfg(debug_assertions)]
  {
    dotenv::dotenv()?;
    flo_log_subscriber::init_env_override("flo_node_service=debug,flo_node=debug");
  }

  serve().await?;

  Ok(())
}
