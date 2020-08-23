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
