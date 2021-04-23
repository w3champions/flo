use std::sync::Once;
pub use tracing::{debug, error, info, instrument, span, warn, Level};
pub use tracing_futures::Instrument;

static INIT: Once = Once::new();

pub fn init() {
  INIT.call_once(|| {
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt::init();

    #[cfg(not(debug_assertions))]
    tracing_subscriber::fmt::fmt().with_ansi(false).init();
  });
}

pub fn init_env_override(env: &str) {
  std::env::set_var("RUST_LOG", env);
  init();
}
