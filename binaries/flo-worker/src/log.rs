use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;

#[cfg(debug_assertions)]
pub fn init(debug: bool) {
  let filter = EnvFilter::from_default_env()
    // Set the base level when not matched by other directives to WARN.
    .add_directive(if debug {
      LevelFilter::DEBUG.into()
    } else {
      LevelFilter::INFO.into()
    });

  tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg(not(debug_assertions))]
pub fn init(debug: bool) {
  use once_cell::sync::OnceCell;
  use tracing_appender::non_blocking::WorkerGuard;
  static INSTANCE: OnceCell<WorkerGuard> = OnceCell::new();

  let file_appender = tracing_appender::rolling::daily("flo-logs", "flo.log");
  let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

  INSTANCE.set(guard).unwrap();

  let filter = EnvFilter::from_default_env()
    // Set the base level when not matched by other directives to WARN.
    .add_directive(if debug {
      LevelFilter::DEBUG.into()
    } else {
      LevelFilter::INFO.into()
    });

  tracing_subscriber::fmt()
    .with_env_filter(filter)
    .with_writer(non_blocking)
    .with_ansi(false)
    .init();

  tokio::spawn(async {
    start_log_vacuum("flo-logs").await.map_err(|err| {
      tracing::error!("log_vacuum: {}", err);
    })
  });
}

#[cfg(not(debug_assertions))]
async fn start_log_vacuum(path: &str) -> anyhow::Result<()> {
  use std::time::{Duration, SystemTime};
  use tokio::fs;

  const MAX_RETENTION: Duration = Duration::from_secs(3 * 3600 * 24);
  const RUN_INTERVAL: Duration = Duration::from_secs(3600);

  loop {
    let mut dir = fs::read_dir(path).await?;
    while let Some(entry) = dir.next_entry().await? {
      if let Ok(meta) = entry.metadata().await {
        if let Ok(created) = meta.created() {
          if let Ok(d) = SystemTime::now().duration_since(created) {
            if d > MAX_RETENTION {
              fs::remove_file(entry.path()).await.ok();
            }
          }
        }
      }
    }
    tokio::time::sleep(RUN_INTERVAL).await;
  }
}

#[cfg(not(debug_assertions))]
#[tokio::test]
async fn test_log_vacuum() {
  start_log_vacuum("/Applications/w3champions.app/Contents/Resources/app.asar.unpacked/flo-logs")
    .await
    .unwrap();
}
