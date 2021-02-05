use crate::{
  STATISTIC_SERVICE,
  types::w3c::*
};

use anyhow;
use ureq;

pub fn get_race_flo(r : u32) -> String {
  String::from(
    match r { 0 => "H"
            , 1 => "O"
            , 2 => "NE"
            , 3 => "UD"
            , _ => "RND"})
}

pub fn flo_to_w3c_race(r: u32) -> u32 {
  match r { 0 => 1
          , 1 => 2
          , 2 => 4
          , 3 => 8
          , _ => 0 }
}

pub fn get_league(l: u32) -> String {
  String::from(match l { 0 => "GrandMaster"
                       , 1 => "Master"
                       , 2 => "Diamond"
                       , 3 => "Platinum"
                       , 4 => "Gold"
                       , 5 => "Silver"
                       , 6 => "Bronze"
                       , _ => "" })
}

pub fn get_current_season() -> anyhow::Result<u32> {
  let seasons_uri = format!("{}/ladder/seasons", STATISTIC_SERVICE);
  let seasons = ureq::get(&seasons_uri)
                     .call()?.into_json::<Vec<Season>>()?;
  let seasons_ids = seasons.iter().map(|s| s.id);
  if let Some(last_season) = seasons_ids.max() {
    Ok(last_season)
  } else {
    Ok(5) // some default value
  }
}

pub fn get_player(target: &str, season: u32) -> anyhow::Result<Option<String>> {
  if target.contains('#') {
    Ok(Some(target.to_string()))
  }
  else {
    let search_uri =
      format!("{}/ladder/search?gateWay=20&searchFor={}&season={}", STATISTIC_SERVICE, target, season);
    let search: Vec<Search> = ureq::get(&search_uri).call()?.into_json::<Vec<Search>>()?;
    if !search.is_empty() {
      // search for ToD will give you Toddy at first, so we search for exact match
      for s in &search {
        for id in &s.player.playerIds {
          if target == id.name {
            return Ok(Some(id.battleTag.clone()));
          }
        }
      }
      // if there is no exact match return first search result
      if let Some(player_id) = search[0].player.playerIds.get(0) {
        Ok(Some(player_id.battleTag.clone()))
      } else {
        Ok(None)
      }
    } else {
      Ok(None)
    }
  }
}
