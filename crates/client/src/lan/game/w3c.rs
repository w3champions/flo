use anyhow;
use ureq;
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Deserialize)]
pub struct PlayerId {
  pub name: String,
  pub battleTag: String
}

#[derive(Deserialize)]
pub struct W3CPlayer {
  pub playerIds: Vec<PlayerId>,
  pub name: String,
  pub id: String,
  pub mmr: u32,
  pub gateWay: u32,
  pub gameMode: u32,
  pub season: u32,
  pub wins: u32,
  pub losses: u32,
  pub games: u32,
  pub winrate: f64
}

#[derive(Deserialize)]
pub struct Search {
  pub gateway: u32,
  pub id: String,
  pub league: u32,
  pub rankNumber: u32,
  pub rankingPoints: u32,
  pub playerId: String,
  pub player: W3CPlayer,
  pub gameMode: u32,
  pub season: u32
}

#[derive(Deserialize)]
pub struct RankingPointsProgress {
  pub rankingPoints: i32,
  pub mmr: i32
}

#[derive(Deserialize)]
pub struct GMStats {
  pub race: Option<u32>,
  pub division: u32,
  pub gameMode: u32,
  pub games: u32,
  pub gateWay: u32,
  pub id: String,
  pub leagueId: u32,
  pub leagueOrder: u32,
  pub losses: u32,
  pub mmr: u32,
  pub playerIds: Vec<PlayerId>,
  pub rank: u32,
  pub rankingPoints: u32,
  pub rankingPointsProgress: RankingPointsProgress,
  pub season: u32,
  pub winrate: f64,
  pub wins: u32
}

fn get_league(l: u32) -> String {
  String::from(match l { 0 => "GrandMaster"
                       , 1 => "Master"
                       , 2 => "Diamond"
                       , 3 => "Platinum"
                       , 4 => "Gold"
                       , 5 => "Silver"
                       , 6 => "Bronze"
                       , _ => "" })
}

pub fn search(target: &str) -> anyhow::Result<String> {
  let season = 5;
  let search_uri =format!("https://statistic-service.w3champions.com/api/ladder/search?gateWay=20&searchFor={}&season={}", target, season);
  let search: Vec<Search> = ureq::get(&search_uri).call()?.into_json::<Vec<Search>>()?;
  let mut league_info = String::new();
  if !search.is_empty() {
    if !search[0].player.playerIds.is_empty() {
      let user = search[0].player.playerIds[0].battleTag.clone().replace("#","%23");
      let game_mode_uri = format!("https://statistic-service.w3champions.com/api/players/{}/game-mode-stats?season={}&gateWay=20", user, season);
      let game_mode_stats: Vec<GMStats> = ureq::get(&game_mode_uri).call()?.into_json::<Vec<GMStats>>()?;
      for gmstat in game_mode_stats {
        // solo games
        if gmstat.gameMode == 1 && league_info.is_empty() {
          let winrate = (gmstat.winrate * 100.0).round();
          let progr = if gmstat.rankingPointsProgress.mmr > 0 {
              format!("+{}", gmstat.rankingPointsProgress.mmr)
            } else {
              gmstat.rankingPointsProgress.mmr.to_string()
            };
          let league_str = get_league(gmstat.leagueOrder);
          let league_division = if gmstat.games < 5 {
              String::from("Calibrating")
            } else {
              if gmstat.leagueOrder > 1 {
                format!("*League*: **{}** *Division:* **{}**", league_str, gmstat.division)
              } else {
                format!("*League*: **{}**", league_str)
              }
            };
          league_info = format!("**Winrate**: **{}%** **MMR**: __**{}**__ (*{}*)\n{} *Rank*: **{}**",
            winrate, gmstat.mmr, progr, &league_division, gmstat.rank);
        }
      }
    }
  }
  if league_info.is_empty() {
    Ok(format!("No stats found for {}", target))
  } else {
    Ok(league_info)
  }
}
