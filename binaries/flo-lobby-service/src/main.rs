use bs_diesel_utils::Executor;
use flo_lobby::{serve_grpc, serve_socket, ApiClientStorage, StateStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenv::dotenv()?;

  let db = Executor::env().into_ref();
  let state = StateStorage::init(db.clone()).await?;
  let api_client = ApiClientStorage::init(db.clone()).await?.into_ref();

  tokio::try_join!(
    serve_grpc(db.clone(), state.handle(), api_client.clone()),
    serve_socket(db.clone(), state.handle())
  )?;

  Ok(())
}
