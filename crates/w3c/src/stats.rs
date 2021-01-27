use crate::{
  types::w3c::*,
  utils::*
};

use anyhow;
use ureq;

pub fn get_stats(target: &str, race: u32) -> anyhow::Result<String> {
  let season = 5;
  let mut league_info = String::new();
  let race_str = get_race_flo(race);
  if let Some(player) = get_player(target, season)? {
    let name = &player.split('#').collect::<Vec<&str>>()[0];
    let user = player.replace("#","%23");
    let game_mode_uri = format!("https://statistic-service.w3champions.com/api/players/{}/game-mode-stats?season={}&gateWay=20", user, season);
    let game_mode_stats: Vec<GMStats> = ureq::get(&game_mode_uri).call()?.into_json::<Vec<GMStats>>()?;
    for gmstat in game_mode_stats {
      // for now displaying only solo games
      if gmstat.gameMode == 1 && league_info.is_empty() && gmstat.race.is_some()
        && gmstat.race.unwrap() == flo_to_w3c_race(race) {
        let winrate = (gmstat.winrate * 100.0).round();
        let league_str = get_league(gmstat.leagueOrder);
        let league_division = if gmstat.games < 5 {
            String::from("Calibrating")
          } else {
            if gmstat.leagueOrder > 1 {
              format!("{} {}", league_str, gmstat.division)
            } else {
              format!("{}", league_str)
            }
          };
        league_info = format!("{} ({}): {} Rank: {} Games {}-{} Winrate: {}%, MMR: {}",
          name, race_str
              , &league_division
              , gmstat.wins
              , gmstat.losses
              , gmstat.rank
              , winrate
              , gmstat.mmr);
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
  let tod = get_stats("ToD", 0);
  assert!(tod.is_ok());
  let string_tod = tod.unwrap();
  assert!(!string_tod.is_empty());
  let also_tod = get_stats("ToD#2792", 0).unwrap();
  assert_eq!(string_tod, also_tod);
}
