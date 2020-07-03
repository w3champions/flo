pub use tracing::{debug, error, info, instrument, span, warn, Level};
pub use tracing_futures::Instrument;

pub fn init_debug(mod_path: &str) {
  std::env::set_var("RUST_LOG", format!("{}=debug", mod_path));
  tracing_subscriber::fmt::init();
}
