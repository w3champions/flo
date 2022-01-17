use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  Token { game_id: i32 },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::Token { game_id } => {
        let token = flo_observer::token::create_observer_token(game_id, None).unwrap();
        println!("{}", token)
      }
    }

    Ok(())
  }
}
