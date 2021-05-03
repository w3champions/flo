use crate::Result;
use flo_debug::player_emulator::PlayerEmulator;
use flo_lan::search_lan_games;
use flo_w3storage::W3Storage;
use futures::StreamExt;
use std::time::Duration;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum Command {
  List,
  Join { name: String },
  JoinMulti { name: String, player_ids: Vec<i32> },
}

impl Command {
  pub async fn run(&self) -> Result<()> {
    match *self {
      Command::List => {
        let games = search_lan_games(Duration::from_secs(3)).await;
        for game in games {
          println!("{}", game.game_info.name.to_string_lossy());
        }
      }
      Command::Join { ref name } => {
        let storage = W3Storage::from_env()?;
        let games = search_lan_games(Duration::from_secs(3)).await;
        let game = games
          .iter()
          .find(|g| g.game_info.name.as_bytes() == name.as_bytes())
          .unwrap();
        let map = PlayerEmulator::check_map(game, &storage)?;
        let emu = PlayerEmulator::join(game, map, "Fake Player").await?;
        let handle = emu.handle();
        let ctrl_c = tokio::signal::ctrl_c();
        let run = emu.run();
        tokio::pin!(run, ctrl_c);
        let mut leaving = false;
        loop {
          tokio::select! {
            _ = &mut run => break,
            _ = &mut ctrl_c, if !leaving => {
              tracing::info!("leaving");
              leaving = true;
              tokio::time::sleep(Duration::from_millis(1000)).await;
              handle.leave().await;
            }
          }
        }
      }
      Command::JoinMulti {
        ref name,
        ref player_ids,
      } => {
        tracing::info!("join multi: name = {}, player_ids = {:?}", name, player_ids);
        let storage = W3Storage::from_env()?;
        let games = search_lan_games(Duration::from_secs(5)).await;
        let games = games
          .iter()
          .filter_map(|g| {
            if g.game_info.name.as_bytes().starts_with(name.as_bytes()) {
              let name = g.game_info.name.to_string_lossy();
              let parts = name.rsplitn(2, "-").collect::<Vec<_>>();
              assert_eq!(parts.len(), 2);
              Some((g.clone(), parts[0].parse::<i32>().unwrap()))
            } else {
              None
            }
          })
          .collect::<Vec<_>>();
        let handles = futures::stream::FuturesUnordered::new();
        let mut map = None;
        for player_id in player_ids.into_iter().cloned() {
          let game = games.iter().find(|t| t.1 == player_id).unwrap().0.clone();
          tracing::info!("join player: {}", player_id);
          let map = map
            .get_or_insert_with(|| PlayerEmulator::check_map(&game, &storage).unwrap())
            .clone();
          let handle = tokio::spawn(async move {
            let emu = PlayerEmulator::join(&game, map, &format!("{}", player_id)).await?;
            emu.run().await?;
            Ok::<_, anyhow::Error>(())
          });
          handles.push(handle);
        }
        let res = handles.collect::<Vec<_>>().await;
        let res = res.into_iter().collect::<Result<Vec<_>, _>>()?;
        for res in res {
          res.unwrap()
        }
      }
    }

    Ok(())
  }
}
