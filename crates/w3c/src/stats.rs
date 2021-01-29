use crate::{
  types::w3c::*,
  utils::*
};

use anyhow;
use ureq;

use once_cell::sync::Lazy;

static STATISTIC_SERVICE: &str = "https://statistic-service.w3champions.com/api";

pub fn get_stats(target: &str, race: u32, solo: bool) -> anyhow::Result<String> {
  static SEASON: Lazy<u32> =
    Lazy::new(|| { get_current_season().unwrap_or(5) });
  let mut league_info = String::new();
  let race_str = get_race_flo(race);
  if let Some(player) = get_player(target, *SEASON)? {
    let name = &player.split('#').collect::<Vec<&str>>()[0];
    let user = player.replace("#","%23");
    let game_mode_uri = format!("{}/players/{}/game-mode-stats?season={}&gateWay=20", STATISTIC_SERVICE, user, *SEASON);
    let game_mode_stats: Vec<GMStats> = ureq::get(&game_mode_uri).call()?.into_json::<Vec<GMStats>>()?;
    let w3c_race = flo_to_w3c_race(race);
    for gmstat in game_mode_stats {
      // for now displaying only solo games
      if gmstat.gameMode == 1 && league_info.is_empty() && gmstat.race.is_some()
        && gmstat.race.unwrap() == w3c_race {
        let winrate = (gmstat.winrate * 100.0).round();
        let league_str = get_league(gmstat.leagueOrder);
        let league_division = if gmstat.games < 5 {
            String::from("Calibrating")
          } else {
            if gmstat.leagueOrder > 1 {
              format!("{} {} Rank: {}", league_str, gmstat.division, gmstat.rank)
            } else {
              format!("{} Rank: {}", league_str, gmstat.rank)
            }
          };
        league_info = format!("{} ({}): {} Games {}-{} Winrate: {}%, MMR: {}",
          name, race_str
              , &league_division
              , gmstat.wins
              , gmstat.losses
              , winrate
              , gmstat.mmr);
      }
    }
    // if person doesn't play solo and it's not a solo game
    // we just grab race statistics
    if league_info.is_empty() && !solo {
      let race_uri = format!("{}/players/{}/race-stats?season={}&gateWay=20", STATISTIC_SERVICE, user, *SEASON);
      let race_stats: Vec<Stats> = ureq::get(&race_uri).call()?.into_json::<Vec<Stats>>()?;
      for stats in race_stats {
        if stats.race == w3c_race {
          let winrate = (stats.winrate * 100.0).round();
          league_info = format!("{} ({}): Games {}-{} Winrate: {}%",
            name, race_str
                , stats.wins
                , stats.losses
                , winrate);
        }
      }
    }
  }
  if league_info.is_empty() {
    Ok(format!("{} ({}): no stats found", target, race_str))
  } else {
    Ok(league_info)
  }
}

#[test]
fn test_get_stats() {
  let tod = get_stats("ToD", 0, true);
  assert!(tod.is_ok());
  let string_tod = tod.unwrap();
  assert!(!string_tod.is_empty());
  let also_tod = get_stats("ToD#2792", 0, true).unwrap();
  assert_eq!(string_tod, also_tod);
}
