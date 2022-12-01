use crate::error::{Error, Result};

pub mod publisher;
pub mod search;

pub(crate) fn get_reg_type(game_version: &str) -> Result<String> {
  let minor = game_version
    .split(".")
    .skip(1)
    .take(1)
    .next()
    .ok_or_else(|| Error::InvalidVersionString(game_version.to_string()))?;
  let num = format!("100{minor}")
    .parse::<i64>()
    .map_err(|_| Error::InvalidVersionString(game_version.to_string()))?;

  Ok(format!("_blizzard._udp,_w3xp{:x}", num))
}

#[test]
fn test_get_reg_type() {
  assert_eq!(
    get_reg_type("1.33.0.00000").unwrap(),
    "_blizzard._udp,_w3xp2731"
  );
  assert_eq!(
    get_reg_type("1.34.0.00000").unwrap(),
    "_blizzard._udp,_w3xp2732"
  );
}
