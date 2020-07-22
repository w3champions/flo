use flo_lobby::{serve_grpc, serve_socket, LobbyStateRef};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenv::dotenv()?;

  flo_log::init_env("flo_lobby_service=debug,flo_lobby=debug");

  let state = LobbyStateRef::init().await?;

  tokio::try_join!(serve_grpc(state.clone()), serve_socket(state.clone()))?;

  Ok(())
}
