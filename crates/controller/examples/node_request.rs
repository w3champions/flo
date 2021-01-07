use flo_controller::error::{Error, Result, TaskCancelledExt};
use flo_controller::node::messages::NodePlayerLeave;
use flo_controller::node::{NodeConnActor, NodeConnConfig};
use flo_state::{mock::Mock, Actor};
use futures::TryFutureExt;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
  dotenv::dotenv().ok();
  flo_log_subscriber::init_env_override("flo_controller=debug");

  let game_registry_mock = Mock::builder().build();

  let actor = NodeConnActor::new(
    NodeConnConfig {
      id: 0,
      addr: "127.0.0.1".to_string(),
      secret: "".to_string(),
    },
    game_registry_mock.addr(),
  )
  .start();

  let tasks: futures::stream::FuturesUnordered<_> = (0..1000)
    .into_iter()
    .map(|id| {
      let addr = actor.addr();
      async move {
        let reply = addr
          .send(NodePlayerLeave {
            game_id: id,
            player_id: 0,
          })
          .await??;

        let reply = reply.await.or_cancelled()?;

        tracing::debug!(id, "done: {:?}", reply);

        Ok::<_, Error>(reply)
      }
      .map_err(move |err| {
        tracing::error!(id, "error: {:?}", err);
        err
      })
    })
    .collect();

  let results = tasks.collect::<Vec<_>>().await;
  results.into_iter().collect::<Result<Vec<_>>>()?;
  Ok(())
}
