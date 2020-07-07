use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

pub const SIGNATURE: [u8; 28] = *b"Warcraft III recorded game\x1A\0";
pub const SUPPORTED_BLOCK_SIZE: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum RecordTypeId {
  #[bin(value = 0x10)]
  GameInfo,
  #[bin(value = 0x16)]
  PlayerInfo,
  #[bin(value = 0x17)]
  PlayerLeft,
  #[bin(value = 0x19)]
  SlotInfo,
  #[bin(value = 0x1A)]
  CountDownStart,
  #[bin(value = 0x1B)]
  CountDownEnd,
  #[bin(value = 0x1C)]
  GameStart,
  #[bin(value = 0x1E)]
  TimeSlotFragment,
  #[bin(value = 0x1F)]
  TimeSlot,
  #[bin(value = 0x20)]
  ChatMessage,
  #[bin(value = 0x22)]
  TimeSlotAck,
  #[bin(value = 0x23)]
  Desync,
  #[bin(value = 0x2F)]
  EndTimer,
  #[bin(value = 0x39)]
  ProtoBuf,
  UnknownValue(u8),
}
