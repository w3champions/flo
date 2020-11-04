pub use tokio::runtime::Runtime;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

pub fn init() -> Result<Runtime> {
  std::panic::catch_unwind(|| -> Result<_> {
    let mut rt = Runtime::new()?;
    let task = rt.block_on(flo_client::start())?;
    rt.spawn(task);
    Ok(rt)
  })
  .map_err(|err| {
    tracing::error!("start panicked: {:?}", err);
    anyhow::format_err!("Start flo client failed.")
  })
  .and_then(std::convert::identity)
}
