pub use tracing::{debug, error, info, instrument, span, warn, Level};
pub use tracing_futures::Instrument;

pub fn init_env(env: &str) {
  std::env::set_var("RUST_LOG", env);
  tracing_subscriber::fmt::init();
}

#[macro_export]
macro_rules! result_ok {
  ($prefix:literal, $result:expr) => {
    match $result {
      Ok(value) => Some(value),
      Err(err) => {
        tracing::error!("{}: {}", $prefix, err);
        None
      }
    }
  };
}
