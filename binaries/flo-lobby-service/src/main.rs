use bs_diesel_utils::Executor;
use flo_grpc::lobby::flo_lobby_server::FloLobbyServer;
use flo_lobby::{ApiClientStorage, FloLobbyService, StateStorage};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenv::dotenv()?;
  let addr = "[::1]:4095".parse()?;

  let db = Executor::env().into_ref();
  let state_storage = StateStorage::init(db.clone()).await?;
  let api_client_storage = ApiClientStorage::init(db.clone()).await?.into_ref();

  let server_impl = FloLobbyService::new(db, state_storage.handle());
  let server =
    FloLobbyServer::with_interceptor(server_impl, api_client_storage.clone().into_interceptor());

  Server::builder().add_service(server).serve(addr).await?;

  Ok(())
}
