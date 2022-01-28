use std::{io::Write, path::PathBuf};

use flo_kinesis::{data_stream::DataStream, iterator::ShardIteratorType};
use flo_net::w3gs::W3GSPacketTypeId;
use flo_observer::record::GameRecordData;
use flo_w3gs::action::IncomingAction;
use std::collections::BTreeMap;
use structopt::StructOpt;

use crate::Result;

#[derive(Debug, StructOpt)]
pub enum Command {
  Dump { secs: u64, dst: PathBuf },
  Inspect { path: PathBuf },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::Dump { secs, ref dst } => {
        use std::time::Duration;
        use tokio_stream::StreamExt;

        std::fs::create_dir_all(dst)?;

        let mut map = BTreeMap::new();

        let mut s = DataStream::from_env()
          .into_iter(ShardIteratorType::at_timestamp_backward(
            Duration::from_secs(secs),
          ))
          .await
          .unwrap();

        while let Some(chunk) = s.next().await {
          tracing::info!(
            "chunk: len = {}, millis_behind_latest = {:?}",
            chunk.game_records.len(),
            chunk.millis_behind_latest
          );

          for (game_id, chunk) in chunk.game_records {
            let file = map
              .entry(game_id)
              .or_insert_with(|| std::fs::File::create(dst.join(game_id.to_string())).unwrap());
            let mut buf: Vec<u8> = vec![];
            for record in chunk.records {
              record.encode(&mut buf);
            }
            tracing::info!(game_id, "writing {} bytes...", buf.len());
            file.write_all(&buf)?;
          }

          if chunk.millis_behind_latest.unwrap() < 100 {
            break;
          }
        }
      }
      Command::Inspect { ref path } => {
        use bytes::Buf;
        let data = std::fs::read(path)?;
        let mut records = vec![];
        let mut buf = data.as_slice();
        let mut time: u64 = 0;
        while buf.remaining() > 0 {
          records.push(GameRecordData::decode(&mut buf)?)
        }
        tracing::info!("found {} records", records.len());
        for record in &records {
          if let GameRecordData::W3GS(pkt) = record {
            match pkt.type_id() {
              W3GSPacketTypeId::IncomingAction | W3GSPacketTypeId::IncomingAction2 => {
                let time_increment_ms =
                  IncomingAction::peek_time_increment_ms(pkt.payload.as_ref())
                    .ok()
                    .unwrap_or_default();
                time += time_increment_ms as u64;
              }
              _ => {}
            }
          }
        }
        tracing::info!("duration: {}ms", time);
      }
    }

    Ok(())
  }
}
