use bitflags::bitflags;

use flo_util::{BinDecode, BinEncode};

// W3GS packet type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum PacketTypeId {
  #[bin(value = 0x01)]
  PingFromHost,
  #[bin(value = 0x04)]
  SlotInfoJoin,
  #[bin(value = 0x05)]
  RejectJoin,
  #[bin(value = 0x06)]
  PlayerInfo,
  #[bin(value = 0x07)]
  PlayerLeft,
  #[bin(value = 0x08)]
  PlayerLoaded,
  #[bin(value = 0x09)]
  SlotInfo,
  #[bin(value = 0x0A)]
  CountDownStart,
  #[bin(value = 0x0B)]
  CountDownEnd,
  #[bin(value = 0x0C)]
  IncomingAction,
  #[bin(value = 0x0D)]
  Desync,
  #[bin(value = 0x0F)]
  ChatFromHost,
  #[bin(value = 0x10)]
  StartLag,
  #[bin(value = 0x11)]
  StopLag,
  #[bin(value = 0x14)]
  GameOver,
  #[bin(value = 0x1C)]
  PlayerKicked,
  #[bin(value = 0x1B)]
  LeaveAck,
  #[bin(value = 0x1E)]
  ReqJoin,
  #[bin(value = 0x21)]
  LeaveReq,
  #[bin(value = 0x23)]
  GameLoadedSelf,
  #[bin(value = 0x26)]
  OutgoingAction,
  #[bin(value = 0x27)]
  OutgoingKeepAlive,
  #[bin(value = 0x28)]
  ChatToHost,
  #[bin(value = 0x29)]
  DropReq,
  #[bin(value = 0x2F)]
  SearchGame,
  #[bin(value = 0x30)]
  GameInfo,
  #[bin(value = 0x31)]
  CreateGame,
  #[bin(value = 0x32)]
  RefreshGame,
  #[bin(value = 0x33)]
  DecreateGame,
  #[bin(value = 0x34)]
  ChatFromOthers,
  #[bin(value = 0x35)]
  PingFromOthers,
  #[bin(value = 0x36)]
  PongToOthers,
  #[bin(value = 0x37)]
  ClientInfo,
  #[bin(value = 0x3B)]
  PeerSet,
  #[bin(value = 0x3D)]
  MapCheck,
  #[bin(value = 0x3F)]
  StartDownload,
  #[bin(value = 0x42)]
  MapSize,
  #[bin(value = 0x43)]
  MapPart,
  #[bin(value = 0x44)]
  MapPartOK,
  #[bin(value = 0x45)]
  MapPartError,
  #[bin(value = 0x46)]
  PongToHost,
  #[bin(value = 0x48)]
  IncomingAction2,
  #[bin(value = 0x59)]
  ProtoBuf,
  UnknownValue(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum ProtoBufMessageTypeId {
  #[bin(value = 0x02)]
  Unknown2,
  #[bin(value = 0x03)]
  PlayerProfile,
  #[bin(value = 0x04)]
  PlayerSkins,
  #[bin(value = 0x05)]
  PlayerUnknown5,
  UnknownValue(u8),
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u8))]
pub enum SlotLayout {
  #[bin(value = 0x00)]
  Melee,
  #[bin(value = 0x01)]
  CustomForces,
  #[bin(value = 0x02)]
  FixedPlayerSettings,
  #[bin(value = 0xCC)]
  Ladder,
  UnknownValue(u8),
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u8))]
pub enum SlotStatus {
  #[bin(value = 0)]
  Open,
  #[bin(value = 1)]
  Closed,
  #[bin(value = 2)]
  Occupied,
  UnknownValue(u8),
}

bitflags! {
  pub struct RacePref: u8 {
      const HUMAN = 0x01;
      const ORC = 0x02;
      const NIGHTELF = 0x04;
      const UNDEAD = 0x08;
      const DEMON = 0x10;
      const RANDOM = 0x20;
      const SELECTABLE = 0x40;
  }
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u8))]
pub enum AI {
  #[bin(value = 0)]
  ComputerEasy,
  #[bin(value = 1)]
  ComputerNormal,
  #[bin(value = 2)]
  ComputerInsane,
  UnknownValue(u8),
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u32))]
pub enum RejectJoinReason {
  #[bin(value = 0x07)]
  JoinInvalid,
  #[bin(value = 0x09)]
  JoinFull,
  #[bin(value = 0x0A)]
  JoinStarted,
  #[bin(value = 0x1B)]
  JoinWrongKey,
  UnknownValue(u32),
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u32))]
pub enum LeaveReason {
  #[bin(value = 0x01)]
  LeaveDisconnect,
  #[bin(value = 0x07)]
  LeaveLost,
  #[bin(value = 0x08)]
  LeaveLostBuildings,
  #[bin(value = 0x09)]
  LeaveWon,
  #[bin(value = 0x0A)]
  LeaveDraw,
  #[bin(value = 0x0B)]
  LeaveObserver,
  #[bin(value = 0x0C)]
  LeaveInvalidSaveGame, // (?)
  #[bin(value = 0x0D)]
  LeaveLobby,
  UnknownValue(u32),
}

#[derive(Debug, Clone, Copy, BinEncode, BinDecode, PartialEq)]
#[bin(enum_repr(u8))]
pub enum MessageType {
  #[bin(value = 0x10)]
  Chat,
  #[bin(value = 0x11)]
  TeamChange,
  #[bin(value = 0x12)]
  ColorChange,
  #[bin(value = 0x13)]
  RaceChange,
  #[bin(value = 0x14)]
  HandicapChange,
  #[bin(value = 0x20)]
  Scoped,
  UnknownValue(u8),
}

bitflags! {
  #[derive(Default)]
  pub struct GameFlags: u32 {
      const CUSTOM_GAME = 0x000001;
      const SINGLE_PLAYER = 0x000005;

      const LADDER_1V1 = 0x000010;
      const LADDER_2V2 = 0x000020;
      const LADDER_3V3 = 0x000040;
      const LADDER_4V4 = 0x000080;

      const SAVED_GAME = 0x000200;
      const TYPE_MASK  = 0x0002F5;

      const SIGNED_MAP = 0x000008;
      const PRIVATE_GAME = 0x000800;

      const CREATOR_USER     = 0x002000;
      const CREATOR_BLIZZARD = 0x004000;
      const CREATOR_MASK     = 0x006000;

      const MAP_TYPE_MELEE    = 0x008000;
      const MAP_TYPE_SCENARIO = 0x010000;
      const MAP_TYPE_MASK     = 0x018000;

      const SIZE_SMALL  = 0x020000;
      const SIZE_MEDIUM = 0x040000;
      const SIZE_LARGE  = 0x080000;
      const SIZE_MASK   = 0x0E0000;

      const OBS_FULL      = 0x100000;
      const OBS_ON_DEFEAT = 0x200000;
      const OBS_NONE      = 0x400000;
      const OBS_MASK      = 0x700000;
  }
}

bitflags! {
  #[derive(Default)]
  pub struct GameSettingFlags: u32 {
    const SPEED_SLOW   = 0x00000000;
    const SPEED_NORMAL = 0x00000001;
    const SPEED_FAST   = 0x00000002;
    const SPEED_MASK   = 0x0000000F;

    const TERRAIN_HIDDEN   = 0x00000100;
    const TERRAIN_EXPLORED = 0x00000200;
    const TERRAIN_VISIBLE  = 0x00000400;
    const TERRAIN_DEFAULT  = 0x00000800;
    const TERRAIN_MASK     = 0x00000F00;

    const OBS_NONE      = 0x00000000;
    const OBS_ENABLED   = 0x00001000;
    const OBS_ON_DEFEAT = 0x00002000;
    const OBS_FULL      = 0x00003000;
    const OBS_REFEREES  = 0x40000000;
    const OBS_MASK      = 0x40003000;

    const TEAMS_TOGETHER = 0x00004000;
    const TEAMS_FIXED    = 0x00060000;

    const SHARED_CONTROL = 0x01000000;
    const RANDOM_HERO    = 0x02000000;
    const RANDOM_RACE    = 0x04000000;
  }
}
