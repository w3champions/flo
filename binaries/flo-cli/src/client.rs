use structopt::StructOpt;

use crate::env::ENV;
use crate::Result;
use std::time::Duration;

#[derive(Debug, StructOpt)]
pub enum Command {
  Token,
  Connect {
    #[structopt(long)]
    ws: bool,
  },
  WsReconnect {
    port: u16,
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
        tracing::info!("controller host: {}", ENV.controller_host);

        if ws {
          let client = flo_client::start(flo_client::StartConfig {
            controller_host: ENV.controller_host.clone().into(),
            ..Default::default()
          })
          .await?;
          let port = client.port();
          tokio::join!(client.serve(), async move {
            loop {
              if let Err(err) = server_ws(format!("ws://127.0.0.1:{}", port), token.clone()).await {
                tracing::error!("ws broken: {}", err);
              }
              tokio::time::sleep(Duration::from_secs(5)).await;
            }
          });
        } else {
          let client = flo_client::start(flo_client::StartConfig {
            token: Some(token),
            controller_host: ENV.controller_host.clone().into(),
            ..Default::default()
          })
          .await?;
          client.serve().await;
        };
      }
      Command::WsReconnect { port } => {
        server_ws(format!("ws://127.0.0.1:{}", port), token).await?;
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

  tracing::info!("websocket url: {}", url);

  let (mut socket, _) = connect_async(&url).await?;
  let conn_msg = serde_json::to_string(&json!({
    "type": "Connect",
    "token": token
  }))?;
  socket.send(Message::Text(conn_msg)).await?;
  while let Some(msg) = socket.next().await {
    let json = msg?.into_text()?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    if value
      .as_object()
      .and_then(|v| v.get("type"))
      .and_then(|v| v.as_str())
      .map(|v| v.contains("Ping"))
      != Some(true)
    {
      tracing::info!("{}", json);
    }
  }
  Ok(())
}
