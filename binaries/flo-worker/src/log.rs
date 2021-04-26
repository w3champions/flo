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
}
