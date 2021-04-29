use crate::error::Error;
use crate::game_info::GameInfo;
use async_dnssd::{browse, query_record, Type};
use futures::stream::TryStreamExt;
use std::collections::BTreeSet;
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::sync::mpsc::channel;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct LanGame {
  pub game_info: GameInfo,
  pub id: u32,
  pub addr: SocketAddrV4,
}

pub async fn search_lan_games(timeout: Duration) -> Vec<LanGame> {
  let (tx, mut rx) = channel(32);

  let mut task = tokio::spawn(async move {
    let mut browse_stream = browse(super::REG_TYPE);
    let mut found_names = BTreeSet::new();

    while let Some(res) = browse_stream.try_next().await? {
      if found_names.contains(&res.service_name) {
        continue;
      }
      found_names.insert(res.service_name.clone());
      if let Some(resolve) = res.resolve().try_next().await? {
        let host_postfix = ".local.";
        let host = if resolve.host_target.ends_with(host_postfix) {
          (&resolve.host_target[..(resolve.host_target.len() - host_postfix.len())]).to_string()
        } else {
          tracing::error!("unknown host name: {}", resolve.host_target);
          continue;
        };

        let mut query = query_record(&resolve.fullname, Type(66));
        if let Some(query_result) = query.try_next().await? {
          match GameInfo::decode_bytes(&query_result.rdata) {
            Ok(game_info) => {
              let port = game_info.data.port;
              let id = if let Ok(v) = game_info.game_id.parse::<u32>() {
                v
              } else {
                tracing::error!("invalid game id: {}", game_info.game_id);
                continue;
              };

              let addr = tokio::net::lookup_host(format!("{}:{}", host, port))
                .await
                .map_err(|err| {
                  tracing::error!("resolve host name: {}", err);
                  err
                })?
                .filter_map(|addr| match addr {
                  SocketAddr::V4(addr) => Some(addr),
                  SocketAddr::V6(_) => None,
                })
                .next();

              if let Some(addr) = addr {
                if let Err(_) = tx
                  .send(LanGame {
                    game_info,
                    addr,
                    id,
                  })
                  .await
                {
                  break;
                }
              } else {
                tracing::error!("no v4 addr: {}", host)
              }
            }
            Err(err) => {
              tracing::error!("parse game info of `{}`: {}", resolve.fullname, err);
            }
          }
        }
      }
    }

    Ok::<_, Error>(())
  });

  let mut records = vec![];

  let timeout = sleep(timeout);
  tokio::pin!(timeout);
  loop {
    tokio::select! {
      _ = &mut timeout => break,
      res = &mut task => {
        tracing::error!("search task crashed: {:?}", res);
        break;
      }
      Some(item) = rx.recv() => {
        records.push(item);
      }
    }
  }

  records
}

#[tokio::test]
async fn test_search() {
  dbg!(search_lan_games(Duration::from_secs(5)).await);
}
