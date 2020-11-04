use once_cell::sync::OnceCell;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;

static INSTANCE: OnceCell<WorkerGuard> = OnceCell::new();

pub fn init() {
  let file_appender = tracing_appender::rolling::daily("logs", "flo.log");
  let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

  INSTANCE.set(guard).unwrap();

  let filter = EnvFilter::from_default_env()
    // Set the base level when not matched by other directives to WARN.
    .add_directive(LevelFilter::DEBUG.into());

  tracing_subscriber::fmt()
    .with_env_filter(filter)
    .with_writer(non_blocking)
    .with_ansi(false)
    .init();
}
