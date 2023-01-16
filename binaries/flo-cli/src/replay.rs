use flo_w3replay::replay::ReplayDecoder;
use std::path::PathBuf;
use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  DumpHeader { path: PathBuf },
  DumpGameInfo { path: PathBuf },
  DumpSlotInfo { path: PathBuf },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::DumpHeader { ref path } => {
        let d = ReplayDecoder::new(std::fs::File::open(path)?)?;
        println!("{:#?}", d.header())
      }
      Command::DumpGameInfo { ref path } => {
        let d = ReplayDecoder::new(std::fs::File::open(path)?)?;
        for r in d.into_records() {
          let r = r?;
          match r {
            flo_w3replay::Record::GameInfo(game) => {
              println!("{:#?}", game)
            }
            _ => {}
          }
        }
      }
      Command::DumpSlotInfo { ref path } => {
        let d = ReplayDecoder::new(std::fs::File::open(path)?)?;
        for r in d.into_records() {
          let r = r?;
          match r {
            flo_w3replay::Record::SlotInfo(slot) => {
              // println!("{:#?}", slot);
              println!("#\tTEAM\tSTATUS\tTYPE");
              for (i, slot) in slot.slots().into_iter().enumerate() {
                println!(
                  "{}\t{}\t{:?}\t{}",
                  i,
                  slot.team,
                  slot.slot_status,
                  if slot.computer { "COMPUTER" } else { "PLAYER" }
                );
              }
            }
            _ => {}
          }
        }
      }
    }
    Ok(())
  }
}
