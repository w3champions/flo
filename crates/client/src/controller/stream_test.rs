use crate::controller::stream::*;
use crate::controller::{ControllerClient, SendWs};
use crate::error::Result;
use crate::node::UpdateNodes;
use flo_state::mock::Mock;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_controller_stream() {
  dotenv::dotenv().unwrap();
  flo_log_subscriber::init_env_override("DEBUG");
  let _token = flo_controller::player::token::create_player_token(1).unwrap();

  async fn mock_handle_event(msg: ControllerEvent) {
    tracing::debug!("mock: ControllerStreamEvent: {:?}", msg);
  }

  async fn mock_handle_send_ws(msg: SendWs) {
    tracing::debug!("mock: SendWs: {:?}", msg.message);
  }

  async fn mock_handle_update_nodes(msg: UpdateNodes) -> Result<()> {
    tracing::debug!("mock: UpdateNodes: {:?}", msg);
    Ok(())
  }

  let mut parent = Mock::<ControllerClient>::builder()
    .handle(mock_handle_event)
    .handle(mock_handle_send_ws)
    .handle(mock_handle_update_nodes)
    .build();
  
  //TODO: ControllerStream takes 6 arguments, platform and nodes Addr
  //let s = ControllerStream::new(parent.addr(), 1, "127.0.0.1", token).start();

  sleep(Duration::from_secs(1)).await;

  parent.shutdown().await.unwrap();
}
