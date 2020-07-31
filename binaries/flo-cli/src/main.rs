use structopt::StructOpt;

mod client;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, StructOpt)]
enum Opt {
  Client {
    player_id: i32,
    #[structopt(subcommand)]
    cmd: client::Command,
  },
}

#[tokio::main]
async fn main() -> Result<()> {
  dotenv::dotenv()?;
  flo_log::init_env("flo_lobby_service=debug,flo_lobby=debug,flo_cli=debug");

  let opt = Opt::from_args();

  match opt {
    Opt::Client { player_id, cmd } => {
      cmd.run(player_id).await?;
    }
  }

  Ok(())
}
