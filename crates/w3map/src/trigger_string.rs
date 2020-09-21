// The war3map.wts file : The Trigger String Data File

use flo_util::binary::*;
use std::borrow::Cow;
use std::collections::BTreeMap;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Debug)]
pub struct TriggerStringMap(BTreeMap<i32, String>);

impl TriggerStringMap {
  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn get(&self, id: &TriggerStringRef) -> Option<Cow<str>> {
    match *id {
      TriggerStringRef::Null => None,
      TriggerStringRef::Ref(ref id) => self.0.get(id).map(|v| Cow::Borrowed(v.as_ref())),
      TriggerStringRef::Inline(ref v) => Some(Cow::Owned(v.clone())),
    }
  }

  pub fn iter(&self) -> impl Iterator<Item = (TriggerStringRef, &str)> {
    self
      .0
      .iter()
      .map(|(k, v)| (TriggerStringRef::Ref(*k), v.as_ref()))
  }
}

impl BinDecode for TriggerStringMap {
  const MIN_SIZE: usize = 0;
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    if !buf.has_remaining() {
      return Err(BinDecodeError::incomplete());
    }

    if buf.peek_u8() == Some(UTF8_BOM[0]) {
      buf.get_tag(UTF8_BOM)?;
    }

    let mut map = BTreeMap::new();

    while buf.has_remaining() {
      match buf.peek_u8() {
        Some(b'S') => {
          let item = TriggerString::decode(buf)?;
          map.insert(item.id, item.value);
        }
        Some(b'\r') => {
          buf.get_tag(b"\r\n")?;
        }
        Some(b'\n') => {
          buf.advance(1);
        }
        Some(b'/') => {
          buf.get_tag("//")?;
          buf.advance_until_or_eof(b'\n')?;
        }
        Some(b) => return Err(BinDecodeError::failure(format!("unexpected byte: {}", b))),
        None => break,
      }
    }

    Ok(TriggerStringMap(map))
  }
}

#[derive(Debug)]
struct TriggerString {
  pub id: i32,
  pub value: String,
}

impl BinDecode for TriggerString {
  const MIN_SIZE: usize = 7; // '{\r\n' + 'at least 1 char' + '\r\n}'
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.get_tag("STRING ")?;

    let (id, d): (i32, u8) = buf.get_delimited_from_str(|b| b == b'\r' || b == b'\n')?;
    if d == b'\r' {
      buf.get_tag(b"\n")?;
    }

    skip_comment(buf)?;

    buf.get_tag(b"{")?;
    buf.advance_until(b'\n')?;
    let (value, _) = buf.get_delimited_string(b'\n')?;
    buf.advance_until(b'}')?;
    buf.advance_until_or_eof(b'\n')?;
    Ok(TriggerString {
      id,
      value: value.trim().to_string(),
    })
  }
}

fn skip_comment<T: Buf>(buf: &mut T) -> Result<(), BinDecodeError> {
  while buf.has_remaining() {
    match buf.peek_u8() {
      Some(b'{') => break,
      Some(b'\r') => {
        buf.get_tag(b"\r\n")?;
      }
      Some(b'\n') => {
        buf.advance(1);
      }
      Some(b'/') => {
        buf.get_tag("//")?;
        buf.advance_until_or_eof(b'\n')?;
      }
      Some(b) => return Err(BinDecodeError::failure(format!("unexpected byte: {}", b))),
      None => break,
    }
  }
  Ok(())
}

impl AsRef<str> for TriggerString {
  fn as_ref(&self) -> &str {
    self.value.as_ref()
  }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum TriggerStringRef {
  Null,
  Ref(i32),
  Inline(String),
}

impl TriggerStringRef {
  pub fn is_null(&self) -> bool {
    match self {
      TriggerStringRef::Null => true,
      _ => false,
    }
  }
}

impl BinDecode for TriggerStringRef {
  const MIN_SIZE: usize = 1;
  const FIXED_SIZE: bool = false;

  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    match buf.peek_u8() {
      Some(0) => {
        buf.advance(1);
        return Ok(TriggerStringRef::Null);
      }
      None => return Err(BinDecodeError::incomplete()),
      _ => {}
    }

    if buf.bytes().starts_with(b"TRIGSTR_") {
      buf.get_tag("TRIGSTR_")?;
      let (bytes, _) = buf.get_delimited_bytes(b'\0')?;
      if let Some(pos) = bytes.iter().position(|b| *b != b'0') {
        let num_bytes = &bytes[pos..];
        if let Some(s) = std::str::from_utf8(num_bytes).ok() {
          if let Some(id) = s.parse().ok() {
            return Ok(TriggerStringRef::Ref(id));
          }
        }
      } else if !bytes.is_empty() {
        // all '0'
        return Ok(TriggerStringRef::Ref(0));
      }
      Err(BinDecodeError::failure(format!(
        "invalid trigger string ref: {:?}",
        bytes,
      )))
    } else {
      let (bytes, _) = buf.get_delimited_bytes(b'\0')?;
      let value = String::from_utf8(bytes).map_err(|err| {
        BinDecodeError::failure(format!("parse inline trigger string utf8: {:?}", err))
      })?;
      Ok(TriggerStringRef::Inline(value))
    }
  }
}

#[test]
fn test_parse_trigger_string_file() {
  let mut bytes = &include_bytes!("../../../deps/wc3-samples/map/test_tft.wts")[..];
  let map = TriggerStringMap::decode(&mut bytes).unwrap();
  let pairs: Vec<_> = map.iter().collect();
  assert_eq!(
    pairs,
    vec![
      (TriggerStringRef::Ref(1), "Player 1"),
      (TriggerStringRef::Ref(2), "Force 1"),
      (TriggerStringRef::Ref(3), "Player 2"),
      (TriggerStringRef::Ref(4), "Small Wars"),
      (TriggerStringRef::Ref(5), "2"),
      (TriggerStringRef::Ref(7), "Rorslae"),
      (
        TriggerStringRef::Ref(8),
        "Needs 2 people to play, both teams should be evenly balanced."
      ),
    ]
  )
}

#[test]
fn test_parse_trigger_string_item() {
  let mut buf =
    &b"STRING 1\r\n{\r\nNeeds 2 people to play, both teams should be evenly balanced.\r\n}"[..];
  assert_eq!(
    TriggerString::decode(&mut buf).unwrap().as_ref(),
    "Needs 2 people to play, both teams should be evenly balanced."
  );
}

#[test]
fn test_parse_trigger_string_ref() {
  let mut buf =
    &b"STRING 1\r\n{\r\nNeeds 2 people to play, both teams should be evenly balanced.\r\n}"[..];
  assert_eq!(
    TriggerString::decode(&mut buf).unwrap().as_ref(),
    "Needs 2 people to play, both teams should be evenly balanced."
  );
}

#[test]
fn test_parse_inline_trigger_string_ref() {
  use crate::{W3Map, W3Storage};
  let storage = W3Storage::from_env().unwrap();
  let map =
    W3Map::open_storage(&storage, r#"maps\frozenthrone\(2)shrineoftheancients.w3x"#).unwrap();
  dbg!(map);
}
