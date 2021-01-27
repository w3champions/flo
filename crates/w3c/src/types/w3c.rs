use serde::Deserialize;

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