use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  Token,
  Connect,
}

impl Command {
  #[tracing::instrument(skip(self))]
  pub async fn run(&self, player_id: i32) -> Result<()> {
    let token = flo_controller::player::token::create_player_token(player_id)?;
    match *self {
      Command::Token => println!("{}", token),
      Command::Connect => {
        let token = flo_controller::player::token::create_player_token(player_id)?;
        tracing::debug!("token generated: {}", token);
      }
    }

    Ok(())
  }
}
