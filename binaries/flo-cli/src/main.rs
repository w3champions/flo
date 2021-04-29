use structopt::StructOpt;

mod client;
mod env;
mod game;
mod grpc;
mod lan;
mod server;

pub use anyhow::Result;

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
  Lan {
    #[structopt(subcommand)]
    cmd: lan::Command,
  },
}

#[tokio::main]
async fn main() -> Result<()> {
  dotenv::dotenv()?;
  // flo_log_subscriber::init_env_override("debug,h2=error,async_dnssd=error");
  flo_log_subscriber::init();

  let opt = Opt::from_args();

  match opt {
    Opt::Client { player_id, cmd } => {
      cmd.run(player_id).await?;
    }
    Opt::Server { cmd } => {
      cmd.run().await?;
    }
    Opt::Lan { cmd } => {
      cmd.run().await?;
    }
  }

  Ok(())
}
