static STATISTIC_SERVICE: &str = "https://statistic-service.w3champions.com/api";

mod types;
mod utils;
pub mod stats;

#[cfg(feature = "blacklist")]
pub mod blacklist;
