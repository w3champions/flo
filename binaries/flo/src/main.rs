mod error;
mod net;
mod state;
mod version;
mod ws;

use state::FloState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  flo_log::init_env("flo=debug");

  tracing::info!("version: {}", version::FLO_VERSION);

  let state = FloState::init().await?.into_ref();

  let ws = ws::Ws::init(state.clone()).await?;

  ws.serve().await.unwrap();

  Ok(())
}
