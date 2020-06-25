// The war3map.wts file : The Trigger String Data File

use dhost_util::binary::*;
use std::collections::BTreeMap;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Debug)]
pub struct TriggerStringMap(BTreeMap<i32, String>);

impl TriggerStringMap {
  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn get(&self, id: TriggerStringRef) -> Option<&str> {
    let id = id.0?;
    self.0.get(&id).map(AsRef::as_ref)
  }

  pub fn iter(&self) -> impl Iterator<Item = (TriggerStringRef, &str)> {
    self
      .0
      .iter()
      .map(|(k, v)| (TriggerStringRef(Some(*k)), v.as_ref()))
  }
}

impl BinDecode for TriggerStringMap {
  const MIN_SIZE: usize = 0;
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    if !buf.has_remaining() {
      return Err(BinDecodeError::incomplete());
    }

    buf.get_tag(UTF8_BOM)?;

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

impl AsRef<str> for TriggerString {
  fn as_ref(&self) -> &str {
    self.value.as_ref()
  }
}

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
pub struct TriggerStringRef(Option<i32>);

impl TriggerStringRef {
  pub fn is_null(&self) -> bool {
    return self.0.is_none();
  }
}

impl BinDecode for TriggerStringRef {
  const MIN_SIZE: usize = 1;
  const FIXED_SIZE: bool = false;

  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    match buf.peek_u8() {
      Some(0) => return Ok(TriggerStringRef(None)),
      None => return Err(BinDecodeError::incomplete()),
      _ => {}
    }
    buf.get_tag("TRIGSTR_")?;
    let (bytes, _) = buf.get_delimited_bytes(b'\0')?;
    if let Some(pos) = bytes.iter().position(|b| *b != b'0') {
      let num_bytes = &bytes[pos..];
      if let Some(s) = std::str::from_utf8(num_bytes).ok() {
        if let Some(id) = s.parse().ok() {
          return Ok(TriggerStringRef(Some(id)));
        }
      }
    }

    Err(BinDecodeError::failure(format!(
      "invalid trigger string id: {:?}",
      bytes,
    )))
  }
}

#[test]
fn test_parse_trigger_string_file() {
  let mut bytes = &include_bytes!("../samples/test_tft.wts")[..];
  let map = TriggerStringMap::decode(&mut bytes).unwrap();
  let pairs: Vec<_> = map.iter().collect();
  assert_eq!(
    pairs,
    vec![
      (TriggerStringRef(Some(1)), "Player 1"),
      (TriggerStringRef(Some(2)), "Force 1"),
      (TriggerStringRef(Some(3)), "Player 2"),
      (TriggerStringRef(Some(4)), "Small Wars"),
      (TriggerStringRef(Some(5)), "2"),
      (TriggerStringRef(Some(7)), "Rorslae"),
      (
        TriggerStringRef(Some(8)),
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
