use futures::FutureExt;
use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  Token,
  Connect {
    #[structopt(long)]
    ws: bool,
  },
  StartTestGame,
}

impl Command {
  #[tracing::instrument(skip(self))]
  pub async fn run(&self, player_id: i32) -> Result<()> {
    let token = flo_controller::player::token::create_player_token(player_id)?;
    match *self {
      Command::Token => println!("{}", token),
      Command::Connect { ws } => {
        let token = flo_controller::player::token::create_player_token(player_id)?;
        tracing::debug!("token generated: {}", token);

        if ws {
          let client = flo_client::start(flo_client::StartConfig {
            controller_host: "127.0.0.1".to_string().into(),
            ..Default::default()
          })
          .await?;
          let port = client.port();
          tokio::try_join!(
            client.serve().map(Ok),
            server_ws(format!("ws://127.0.0.1:{}", port), token)
          )?;
        } else {
          let client = flo_client::start(flo_client::StartConfig {
            token: Some(token),
            controller_host: "127.0.0.1".to_string().into(),
            ..Default::default()
          })
          .await?;
          client.serve().await;
        };
      }
      Command::StartTestGame => {
        let client = flo_client::start(Default::default()).await.unwrap();
        client.start_test_game().await.unwrap();
        client.serve().await;
      }
    }

    Ok(())
  }
}

async fn server_ws(url: String, token: String) -> Result<()> {
  use async_tungstenite::tokio::connect_async;
  use async_tungstenite::tungstenite::protocol::Message;
  use futures::prelude::*;
  use serde_json::json;
  let (mut socket, _) = connect_async(&url).await?;
  let conn_msg = serde_json::to_string(&json!({
    "type": "Connect",
    "token": token
  }))?;
  socket.send(Message::Text(conn_msg)).await?;
  while let Some(msg) = socket.next().await {
    let json = msg?.into_text()?;
    tracing::info!("{}", json);
  }
  Ok(())
}
