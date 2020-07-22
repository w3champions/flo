use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("parse error: {0}")]
  Parse(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum BinDecodeError {
  #[error("{context}not enough data")]
  Incomplete { context: BinDecodeErrorContext },
  #[error("{context}{message}")]
  Failure {
    message: String,
    context: BinDecodeErrorContext,
  },
}

impl BinDecodeError {
  #[inline]
  pub fn incomplete() -> Self {
    BinDecodeError::Incomplete {
      context: BinDecodeErrorContext::new(),
    }
  }

  #[inline]
  pub fn failure<T>(msg: T) -> Self
  where
    T: std::fmt::Display,
  {
    BinDecodeError::Failure {
      message: msg.to_string(),
      context: BinDecodeErrorContext::new(),
    }
  }

  pub fn context<T: std::fmt::Display>(self, ctx: T) -> Self {
    match self {
      BinDecodeError::Incomplete { mut context } => {
        context.insert(ctx);
        BinDecodeError::Incomplete { context }
      }
      BinDecodeError::Failure {
        message,
        mut context,
      } => {
        context.insert(ctx);
        BinDecodeError::Failure { message, context }
      }
    }
  }

  #[inline]
  pub fn is_incomplete(&self) -> bool {
    match *self {
      BinDecodeError::Incomplete { .. } => true,
      _ => false,
    }
  }
}

#[derive(Debug)]
pub struct BinDecodeErrorContext(Vec<String>);

impl BinDecodeErrorContext {
  fn new() -> Self {
    BinDecodeErrorContext(vec![])
  }

  fn insert<T: std::fmt::Display>(&mut self, ctx: T) {
    self.0.push(ctx.to_string())
  }
}

impl std::fmt::Display for BinDecodeErrorContext {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if self.0.is_empty() {
      return Ok(());
    }

    for ctx in self.0.iter().rev() {
      write!(f, "{}: ", ctx)?
    }

    Ok(())
  }
}
