use anyhow::Result;

static SLED: &str = "blacklist.sled";

pub fn read(target: &str) -> Result<Option<String>> {
  let sled = sled::open(SLED)?;
  if let Some(val) = sled.get(target)? {
    Ok(Some(
      String::from_utf8(val.to_vec())?
    ))
  } else {
    Ok(None)
  }
}

pub fn blacklisted() -> Result<String> {
  let sled = sled::open(SLED)?;
  let mut result = vec![];
  for key in sled.iter().keys() {
    if let Ok(k) = key {
      if let Ok(kk) = String::from_utf8(k.to_vec()) {
        result.push(kk);
      }
    }
  }
  Ok(result.join(", "))
}

pub fn blacklist(target: &str, reason: &str) -> Result<()> {
  let sled = sled::open(SLED)?;
  sled.insert(target, reason)?;
  Ok(())
}

pub fn unblacklist(target: &str) -> Result<()> {
  let sled = sled::open(SLED)?;
  sled.remove(target)?;
  Ok(())
}
