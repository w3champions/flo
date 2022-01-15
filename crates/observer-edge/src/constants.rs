use once_cell::sync::Lazy;

pub static FLO_STATS_MAX_IN_MEMORY_GAMES: Lazy<usize> = Lazy::new(|| {
  std::env::var("FLO_STATS_MAX_IN_MEMORY_GAMES")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(100)
});