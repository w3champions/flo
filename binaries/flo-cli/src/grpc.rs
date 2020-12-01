pub use flo_grpc::controller::flo_controller_client::FloControllerClient;
use flo_grpc::Channel;

pub async fn get_grpc_client() -> FloControllerClient<Channel> {
  let host = std::env::var("FLO_CONTROLLER_HOST")
    .ok()
    .unwrap_or_else(|| "127.0.0.1".to_string());
  let secret = std::env::var("FLO_CONTROLLER_SECRET")
    .ok()
    .unwrap_or_else(|| "TEST".to_string());
  let channel = Channel::from_shared(format!("tcp://{}:3549", host))
    .unwrap()
    .connect()
    .await
    .unwrap();
  FloControllerClient::with_interceptor(channel, move |mut req: tonic::Request<()>| {
    req
      .metadata_mut()
      .insert("x-flo-secret", secret.parse().unwrap());
    Ok(req)
  })
}
