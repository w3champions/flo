use std::borrow::Cow;

pub fn parse_chat_command(value: &[u8]) -> Option<Cow<str>> {
  static PREFIX_LIST: &[u8] = &[b'!', b'-'];
  let start_pos = value.into_iter().position(|c| *c != b' ');
  if let Some(pos) = start_pos {
    if PREFIX_LIST.contains(&value[pos]) {
      Some(String::from_utf8_lossy(&value[(pos + 1)..]))
    } else {
      None
    }
  } else {
    None
  }
}
