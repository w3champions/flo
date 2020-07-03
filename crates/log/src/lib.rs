pub use tracing::{debug, error, info, instrument, span, warn, Level};
pub use tracing_futures::Instrument;

pub fn init_env(env: &str) {
  std::env::set_var("RUST_LOG", env);
  tracing_subscriber::fmt::init();
}
