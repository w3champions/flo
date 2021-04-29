use crate::env::ENV;
pub use flo_grpc::controller::flo_controller_client::FloControllerClient;
use flo_grpc::Channel;

pub async fn get_grpc_client() -> FloControllerClient<Channel> {
  let host = ENV.controller_host.clone();
  let secret = ENV.controller_secret.clone();
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
