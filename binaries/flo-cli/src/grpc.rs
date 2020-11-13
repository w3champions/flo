pub use flo_grpc::controller::flo_controller_client::FloControllerClient;
use flo_grpc::Channel;

pub async fn get_grpc_client() -> FloControllerClient<Channel> {
  let channel = Channel::from_static("tcp://127.0.0.1:3549")
    .connect()
    .await
    .unwrap();
  FloControllerClient::with_interceptor(channel, |mut req: tonic::Request<()>| {
    req
      .metadata_mut()
      .insert("x-flo-secret", "TEST".parse().unwrap());
    Ok(req)
  })
}
