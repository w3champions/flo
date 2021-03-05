use anyhow::{ Result, anyhow };

use once_cell::sync::OnceCell;

static SLED: &str = "blacklist.sled";
static SLED_DB: OnceCell<sled::Db> = OnceCell::new();

fn get_db_handle() -> Result<&'static sled::Db> {
  if let Some(existing_handle) = SLED_DB.get() {
    Ok(existing_handle)
  } else {
    let sled = sled::open(SLED)?;
    SLED_DB.set(sled).map_err(|_| anyhow!("Failed to store db handle"))?;
    get_db_handle()
  }
}

pub fn read(target: &str) -> Result<Option<String>> {
  let sled = get_db_handle()?;
  if let Some(val) = sled.get(target)? {
    Ok(Some(
      String::from_utf8(val.to_vec())?
    ))
  } else {
    Ok(None)
  }
}

pub fn blacklisted() -> Result<String> {
  let sled = get_db_handle()?;
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
  let sled = get_db_handle()?;
  sled.insert(target, reason)?;
  Ok(())
}

pub fn unblacklist(target: &str) -> Result<()> {
  let sled = get_db_handle()?;
  sled.remove(target)?;
  Ok(())
}
