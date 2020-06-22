use bitflags::bitflags;
use lazy_static::lazy_static;

use dhost_util::dword_string::DwordString;

/// W3GS magic number used in the packet header.
pub const PROTOCOL_SIG: u8 = 0xF7;

// W3GS packet type identifier
#[repr(u8)]
pub enum PacketTypeId {
  PingFromHost = 0x01,
  SlotInfoJoin = 0x04,
  RejectJoin = 0x05,
  PlayerInfo = 0x06,
  PlayerLeft = 0x07,
  PlayerLoaded = 0x08,
  SlotInfo = 0x09,
  CountDownStart = 0x0A,
  CountDownEnd = 0x0B,
  IncomingAction = 0x0C,
  Desync = 0x0D,
  ChatFromHost = 0x0F,
  StartLag = 0x10,
  StopLag = 0x11,
  GameOver = 0x14,
  PlayerKicked = 0x1C,
  LeaveAck = 0x1B,
  ReqJoin = 0x1E,
  LeaveReq = 0x21,
  GameLoadedSelf = 0x23,
  OutgoingAction = 0x26,
  OutgoingKeepAlive = 0x27,
  ChatToHost = 0x28,
  DropReq = 0x29,
  SearchGame = 0x2F,
  GameInfo = 0x30,
  CreateGame = 0x31,
  RefreshGame = 0x32,
  DecreateGame = 0x33,
  ChatFromOthers = 0x34,
  PingFromOthers = 0x35,
  PongToOthers = 0x36,
  ClientInfo = 0x37,
  PeerSet = 0x3B,
  MapCheck = 0x3D,
  StartDownload = 0x3F,
  MapSize = 0x42,
  MapPart = 0x43,
  MapPartOK = 0x44,
  MapPartError = 0x45,
  PongToHost = 0x46,
  IncomingAction2 = 0x48,
  PlayerExtra = 0x59,
}

lazy_static! {
  pub static ref PRODUCT_DEMO: DwordString = DwordString::new("W3DM");
  pub static ref PRODUCT_ROC: DwordString = DwordString::new("WAR3");
  pub static ref PRODUCT_TFT: DwordString = DwordString::new("W3XP");
}

bitflags! {
  #[derive(Default)]
  pub struct SlotLayout: u32 {
      const MELEE = 0x00;
      const CUSTOM_FORCES = 0x01;
      const FIXED_PLAYER_SETTINGS = 0x02;
      const LADDER = 0xCC;
  }
}

#[repr(u8)]
pub enum SlotStatus {
  Open = 0x00,
  Closed = 0x01,
  Occupied = 0x02,
}

bitflags! {
  pub struct RacePref: u32 {
      const HUMAN = 0x01;
      const ORC = 0x02;
      const NIGHTELF = 0x04;
      const UNDEAD = 0x08;
      const DEMON = 0x10;
      const RANDOM = 0x20;
      const SELECTABLE = 0x40;
  }
}

#[repr(u8)]
pub enum AI {
  ComputerEasy = 0x00,
  ComputerNormal = 0x01,
  ComputerInsane = 0x02,
}

#[repr(u8)]
pub enum RejectReason {
  JoinInvalid = 0x07,
  JoinFull = 0x09,
  JoinStarted = 0x0A,
  JoinWrongKey = 0x1B,
}

#[repr(u8)]
pub enum LeaveReason {
  LeaveDisconnect = 0x01,
  LeaveLost = 0x07,
  LeaveLostBuildings = 0x08,
  LeaveWon = 0x09,
  LeaveDraw = 0x0A,
  LeaveObserver = 0x0B,
  LeaveInvalidSaveGame = 0x0C, // (?)
  LeaveLobby = 0x0D,
}

#[repr(u8)]
pub enum MessageType {
  Chat = 0x10,
  TeamChange = 0x11,
  ColorChange = 0x12,
  RaceChange = 0x13,
  HandicapChange = 0x14,
  ChatExtra = 0x20,
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
