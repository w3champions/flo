use anyhow::Result;
use crate::types::w3c::Proxy;

pub async fn get_proxies() -> Result<Vec<Proxy>> {
  let url = format!("{}/flo/proxies", crate::MATCHMAKING_SERVICE);
  let res: Vec<Proxy> = reqwest::get(&url).await?.error_for_status()?.json().await?;
  Ok(res)
}

#[tokio::test]
async fn test_get_proxies() {
  get_proxies().await.unwrap();
}