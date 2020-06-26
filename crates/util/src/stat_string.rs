//! Warcraft III stat string
//!
//! [W3G Format](http://w3g.deepnode.de/files/w3g_format.txt)
//! ```text
//! - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//! 4.3 [Encoded String]
//! - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//!
//! There are the GameSetting and three null-terminated strings here back-to-back,
//! all encoded into a single null terminated string.
//! (But we don't know the reason for this encoding!)
//!
//! Every even byte-value was incremented by 1. So all encoded bytes are odd.
//! A control-byte stores the transformations for the next 7 bytes.
//!
//! Since all NullBytes were transformed to 1, they will never occure inside the
//! encoded string. But a NullByte marks the end of the encoded string.
//!
//! The encoded string starts with a control byte.
//! The control byte holds a bitfield with one bit for each byte of the next 7
//! bytes block. Bit 1 (not Bit 0) corresponds to the following byte right after
//! the control-byte, bit 2 to the next, and so on.
//! Only Bit 1-7 contribute to encoded string. Bit 0 is unused and always set.
//!
//! Decoding these bytes works as follows:
//!
//! If the corresponding bit is a '1' then the character is moved over directly.
//! If the corresponding bit is a '0' then subtract 1 from the character.
//!
//! After a control-byte and the belonging 7 bytes follows a new control-byte
//! until you find a NULL character in the stream.
//! ```
//!
//! Ported to Rust from
//! https://github.com/dns/GProxy-Warcraft3-disconnect-protection-tool/blob/master/util.cpp
//!

pub fn encode(src: &[u8]) -> Vec<u8> {
  let mut mask: u8 = 1;
  let mut out = vec![];

  for (i, byte) in src.iter().cloned().enumerate() {
    if byte % 2 == 0 {
      out.push(byte.wrapping_add(1));
    } else {
      out.push(byte);
      mask = mask | (1 << ((i % 7) + 1));
    }

    if i % 7 == 6 || i == src.len() - 1 {
      out.insert(src.len() - 1 - (i % 7), mask);
      mask = 1;
    }
  }

  out
}

pub fn decode(src: &[u8]) -> Vec<u8> {
  let mut mask: u8 = 0;
  let mut out = vec![];

  for (i, byte) in src.iter().cloned().enumerate() {
    if i % 8 == 0 {
      mask = byte;
    } else {
      if (mask & (1 << (i % 8))) == 0 {
        out.push(byte.wrapping_sub(1));
      } else {
        out.push(byte);
      }
    }
  }

  out
}

pub fn encoded_len(data_len: usize) -> usize {
  if data_len % 7 == 0 {
    data_len / 7 * 8
  } else {
    data_len / 7 * 8 + (data_len % 7 + 1)
  }
}

#[test]
fn test_stat_string() {
  let data = "\0".as_bytes();
  let encoded = encode(data);
  assert_eq!(encoded, &[0b00000001, 1]);

  assert_eq!(encoded_len(1), 2);
}
