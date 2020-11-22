use structopt::StructOpt;

mod client;
mod game;
mod grpc;
mod server;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, StructOpt)]
enum Opt {
  Client {
    player_id: i32,
    #[structopt(subcommand)]
    cmd: client::Command,
  },
  Server {
    #[structopt(subcommand)]
    cmd: server::Command,
  },
}

#[tokio::main]
async fn main() -> Result<()> {
  dotenv::dotenv()?;
  flo_log_subscriber::init_env_override("flo_client,flo_cli");

  let opt = Opt::from_args();

  match opt {
    Opt::Client { player_id, cmd } => {
      cmd.run(player_id).await?;
    }
    Opt::Server { cmd } => {
      cmd.run().await?;
    }
  }

  Ok(())
}
