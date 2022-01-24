use structopt::StructOpt;

use crate::{Result, env::ENV};

#[derive(Debug, StructOpt)]
pub enum Command {
  Token { game_id: i32 },
  Watch { game_id: i32 },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::Token { game_id } => {
        let token = flo_observer::token::create_observer_token(game_id, None)?;
        println!("{}", token)
      }
      Command::Watch { game_id } => {
        let token = flo_observer::token::create_observer_token(game_id, None)?;
        let client = flo_client::start(flo_client::StartConfig {
          stats_host: ENV.stats_host.clone().into(),
          ..Default::default()
        })
        .await?;
        client.watch(token).await?;
        client.serve().await;
      }
    }

    Ok(())
  }
}
