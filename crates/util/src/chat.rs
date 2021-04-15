use impl_trait_for_tuples::impl_for_tuples;
use std::borrow::Cow;
use std::str::FromStr;

#[derive(Debug)]
pub enum ChatCommandArgumentParseError<'a> {
  ItemMissing { index: usize },
  ItemParseError { index: usize, value: &'a str },
}

pub type Result<'a, T> = std::result::Result<T, ChatCommandArgumentParseError<'a>>;

pub trait ChatCommandArguments: Sized {
  fn parse(value: &str) -> Result<Self>;
}

impl<T> ChatCommandArguments for Option<T>
where
  T: ChatCommandArguments,
{
  fn parse(value: &str) -> Result<Self> {
    if value.is_empty() {
      return Ok(None);
    }
    Ok(Some(T::parse(value)?))
  }
}

#[impl_for_tuples(1, 5)]
#[tuple_types_custom_trait_bound(FromStr)]
impl ChatCommandArguments for Tuple {
  fn parse(value: &str) -> Result<Self> {
    let mut iter = value.split_whitespace().enumerate();
    let mut last_index = None;
    Ok(for_tuples!(
      (
        #(
          iter.next()
            .map(|(index, value)| {
              last_index.replace(index);
              value.parse::<Tuple>()
                .map_err(|_| ChatCommandArgumentParseError::ItemParseError {
                  index,
                  value,
                })
            })
            .transpose()?
            .ok_or_else(|| {
              ChatCommandArgumentParseError::ItemMissing {
                index: last_index.map(|v| v + 1).unwrap_or(0),
              }
            })?
        ),*
      )
    ))
  }
}

pub struct ChatCommand<'a> {
  name: String,
  arguments: Option<String>,
  raw: Cow<'a, str>,
}

impl<'a> ChatCommand<'a> {
  pub fn name(&self) -> &str {
    self.name.as_str()
  }

  pub fn parse_arguments<T: ChatCommandArguments>(&self) -> Result<T> {
    T::parse(self.arguments.as_ref().map(AsRef::as_ref).unwrap_or(""))
  }

  pub fn raw(&self) -> &str {
    self.raw.as_ref()
  }
}

pub fn parse_chat_command(value: &[u8]) -> Option<ChatCommand> {
  static PREFIX_LIST: &[u8] = &[b'!', b'-'];
  let start_pos = value.into_iter().position(|c| *c != b' ');
  let cmd = if let Some(pos) = start_pos {
    if PREFIX_LIST.contains(&value[pos]) {
      String::from_utf8_lossy(&value[(pos + 1)..])
    } else {
      return None;
    }
  } else {
    return None;
  };

  let name = cmd.split_whitespace().next()?;

  Some(ChatCommand {
    name: name.to_lowercase(),
    arguments: if cmd.len() > name.len() {
      Some((&cmd[name.len()..]).trim().to_string())
    } else {
      None
    },
    raw: cmd,
  })
}

#[test]
fn test_parse_chat_command() {
  let cmd = parse_chat_command(b"!test").unwrap();
  assert_eq!(cmd.name(), "test");
  assert!(cmd.arguments.is_none());

  let cmd = parse_chat_command(b"!Test").unwrap();
  assert_eq!(cmd.name(), "test");
  assert!(cmd.arguments.is_none());

  let cmd = parse_chat_command(b"!Test").unwrap();
  assert_eq!(cmd.name(), "test");
  assert!(cmd.arguments.is_none());
  assert!(cmd.parse_arguments::<(i32,)>().is_err());
  assert_eq!(cmd.parse_arguments::<Option<(i32,)>>().unwrap(), None);

  let cmd = parse_chat_command(b"!test 1 flux 1.0 565656").unwrap();
  assert_eq!(cmd.name(), "test");
  assert_eq!(cmd.arguments.as_ref().unwrap(), "1 flux 1.0 565656");
  let args = cmd.parse_arguments::<(i32, String, f32, u32)>().unwrap();
  assert_eq!(args, (1, "flux".to_string(), 1.0, 565656));
  let args = cmd
    .parse_arguments::<Option<(i32, String, f32, u32)>>()
    .unwrap();
  assert_eq!(args.unwrap(), (1, "flux".to_string(), 1.0, 565656));
}
