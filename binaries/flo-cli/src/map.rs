use flo_w3map::W3Map;
use std::path::PathBuf;
use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  Inspect { path: PathBuf },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::Inspect { ref path } => {
        let (map, checksum) = W3Map::open_with_checksum(path)?;
        println!("Checkdsum: {:?}", checksum);
        println!("Map Name: {}", map.name());
      }
    }
    Ok(())
  }
}
