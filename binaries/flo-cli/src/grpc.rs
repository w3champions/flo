use crate::env::ENV;
pub use flo_grpc::controller::flo_controller_client::FloControllerClient;
use flo_grpc::Channel;
use tonic::service::{Interceptor, interceptor::InterceptedService};

pub async fn get_grpc_client() -> FloControllerClient<InterceptedService<Channel, WithSecret>> {
  let host = ENV.controller_host.clone();
  let channel = Channel::from_shared(format!("tcp://{}:3549", host))
    .unwrap()
    .connect()
    .await
    .unwrap();
   FloControllerClient::with_interceptor(channel, WithSecret)
}

#[derive(Clone)]
pub struct WithSecret;

impl Interceptor for WithSecret {
  fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    req
      .metadata_mut()
      .insert("x-flo-secret", ENV.controller_secret.parse().unwrap());
    Ok(req)
  }
}