#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log_subscriber::init_env_override("flo=debug,flo_lan=debug");
  flo_client::bootstrap().await?;
  tokio::signal::ctrl_c().await?;
  Ok(())
}