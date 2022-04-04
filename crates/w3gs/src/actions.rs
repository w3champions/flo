//! Ported from
//! https://github.com/JSamir/wc3-replay-parser/blob/master/src/main/java/com/w3gdata/parser/action/Actions.java

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum ActionTypeId {
  #[bin(value = 0x01)]
  PauseGame,
  #[bin(value = 0x02)]
  ResumeGame,
  #[bin(value = 0x03)]
  GameSpeed,
  #[bin(value = 0x04)]
  GameSpeedIncreasing,
  #[bin(value = 0x05)]
  GameSpeedDecreasing,
  #[bin(value = 0x06)]
  SaveGame,
  #[bin(value = 0x07)]
  SaveGameFinished,
  #[bin(value = 0x10)]
  UnitBuildingAbility,
  #[bin(value = 0x11)]
  UnitBuildingAbilityTargeted,
  #[bin(value = 0x12)]
  UnitBuildingAbilityTargetedId,
  #[bin(value = 0x13)]
  ItemGivenDropped,
  #[bin(value = 0x14)]
  UnitBuildingAbility2Targets2Items,
  #[bin(value = 0x16)]
  ChangeSelection,
  #[bin(value = 0x17)]
  AssignGroupHotkey,
  #[bin(value = 0x18)]
  SelectGroupHotkey,
  #[bin(value = 0x19)]
  SelectSubgroup114b,
  #[bin(value = 0x1A)]
  PreSubselection,
  #[bin(value = 0x1C)]
  SelectGroundItem,
  #[bin(value = 0x1D)]
  CancelHeroRevival,
  #[bin(value = 0x1E)]
  RemoveUnitFromBuildingQueue,
  #[bin(value = 0x50)]
  ChangeAllyOptions,
  #[bin(value = 0x51)]
  TransferResources,
  #[bin(value = 0x60)]
  MapTriggerChatCommand,
  #[bin(value = 0x61)]
  EscPressed,
  #[bin(value = 0x62)]
  ScenarioTrigger,
  #[bin(value = 0x66)]
  EnterChooseHeroSkillSubmenu,
  #[bin(value = 0x67)]
  EnterChooseBuildingSubmenu,
  #[bin(value = 0x68)]
  MinimapSignal,
  #[bin(value = 0x69)]
  ContinueGameB,
  #[bin(value = 0x6A)]
  ContinueGameA,
  #[bin(value = 0x6B)]
  MMDMessage,
  #[bin(value = 0x1B)]
  Unknown0x1B,
  #[bin(value = 0x21)]
  Unknown0x21,
  #[bin(value = 0x94)]
  Unknown0x94,
  #[bin(value = 0x6C)]
  Unknown0x6C,
  #[bin(value = 0x74)]
  Unknown0x74,
  #[bin(value = 0x75)]
  Unknown0x75,
  UnknownValue(u8),
}

macro_rules! action_enum {
  (
    pub enum Action {
      $(
        $type_id:ident
        $(($data:ty))?
      ),*
    }
  ) => {
    #[derive(Debug)]
    pub enum Action {
      $(
        $type_id
        $(($data))*
        ,
      )*
    }

    impl Action {
      pub fn type_id(&self) -> ActionTypeId {
        match *self {
          $(
            action_enum!(@PATTERN $type_id, $($data),*) =>
            action_enum!(@TYPE_ID $type_id, $($data),*)
          ),*
        }
      }
    }

    impl BinDecode for Action {
      const MIN_SIZE: usize = 1;

      const FIXED_SIZE: bool = false;

      fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
        buf.check_size(1)?;
        let type_id = ActionTypeId::decode(buf)?;
        match type_id {
          $(
            action_enum!(@TYPE_ID $type_id, $($data),*) =>
            action_enum!(@DECODE buf, $type_id, $($data),*),
          )*
          ActionTypeId::UnknownValue(v) => Err(BinDecodeError::failure(format!("unknown action type id: {}", v)))
        }
      }
    }
  };

  (@TYPE_ID $type_id:ident, $data:ty) => {
    ActionTypeId::$type_id
  };

  (@TYPE_ID $type_id:ident,) => {
    ActionTypeId::$type_id
  };

  (@PATTERN $type_id:ident, $data:ty) => {
    Self::$type_id(_)
  };

  (@PATTERN $type_id:ident,) => {
    Self::$type_id
  };

  (@DECODE $buf:expr, $type_id:ident, $data:ty) => {
    Ok(Self::$type_id(<$data>::decode($buf)?))
  };

  (@DECODE $buf:expr, $type_id:ident,) => {
    Ok(Self::$type_id)
  };
}

action_enum! {
  pub enum Action {
    PauseGame,
    ResumeGame,
    GameSpeed(GameSpeed),
    GameSpeedIncreasing,
    GameSpeedDecreasing,
    SaveGame(SaveGame),
    SaveGameFinished(SaveGameFinished),
    UnitBuildingAbility(UnitBuildingAbility),
    UnitBuildingAbilityTargeted(UnitBuildingAbilityTargeted),
    UnitBuildingAbilityTargetedId(UnitBuildingAbilityTargetedId),
    ItemGivenDropped(ItemGivenDropped),
    UnitBuildingAbility2Targets2Items(UnitBuildingAbility2Targets2Items),
    ChangeSelection(ChangeSelection),
    AssignGroupHotkey(AssignGroupHotkey),
    SelectGroupHotkey(SelectGroupHotkey),
    SelectSubgroup114b(SelectSubgroup114b),
    PreSubselection,
    SelectGroundItem(SelectGroundItem),
    CancelHeroRevival(CancelHeroRevival),
    RemoveUnitFromBuildingQueue(RemoveUnitFromBuildingQueue),
    ChangeAllyOptions(ChangeAllyOptions),
    TransferResources(TransferResources),
    MapTriggerChatCommand(MapTriggerChatCommand),
    EscPressed,
    ScenarioTrigger(ScenarioTrigger),
    EnterChooseHeroSkillSubmenu,
    EnterChooseBuildingSubmenu,
    MinimapSignal(MinimapSignal),
    ContinueGameB(Unknown<16>),
    ContinueGameA(Unknown<17>),
    MMDMessage(MMDMessage),
    Unknown0x1B(Unknown<10>),
    Unknown0x21(Unknown<9>),
    Unknown0x94(Unknown<4>),
    Unknown0x6C(Unknown<6>),
    Unknown0x74(Unknown<2>),
    Unknown0x75(Unknown<2>)
  }
}

#[derive(Debug, BinDecode)]
pub struct GameSpeed {
  pub speed: u8,
}

#[derive(Debug, BinDecode)]
pub struct SaveGame {
  pub name: CString,
}

#[derive(Debug, BinDecode)]
pub struct SaveGameFinished {
  _unknown: u32,
}

#[derive(Debug, BinDecode)]
pub struct UnitBuildingAbility {
  pub ability_flag: u16,
  pub item_id: u32,
  _unknown_a: u32,
  _unknown_b: u32,
}

#[derive(Debug, BinDecode)]
pub struct UnitBuildingAbilityTargeted {
  pub ability_flag: u16,
  pub item_id: u32,
  _unknown_a: u32,
  _unknown_b: u32,
  pub target_x: u32,
  pub target_y: u32,
}

#[derive(Debug, BinDecode)]
pub struct UnitBuildingAbilityTargetedId {
  pub ability_flag: u16,
  pub item_id: u32,
  _unknown_a: u32,
  _unknown_b: u32,
  pub target_x: u32,
  pub target_y: u32,
  pub target_object_id_1: u32,
  pub target_object_id_2: u32,
}

#[derive(Debug, BinDecode)]
pub struct ItemGivenDropped {
  pub ability_flag: u16,
  pub item_id: u32,
  _unknown_a: u32,
  _unknown_b: u32,
  pub target_x: u32,
  pub target_y: u32,
  pub target_object_id_1: u32,
  pub target_object_id_2: u32,
  pub item_object_id_1: u32,
  pub item_object_id_2: u32,
}

#[derive(Debug, BinDecode)]
pub struct UnitBuildingAbility2Targets2Items {
  pub ability_flag: u16,
  pub item_id: u32,
  _unknown_a: u32,
  _unknown_b: u32,
  pub target_x: u32,
  pub target_y: u32,
  pub item_2_id: u32,
  _unknown_bytes: [u8; 9],
  pub target_2_x: u32,
  pub target_2_y: u32,
}

#[derive(Debug, BinDecode)]
pub struct ChangeSelection {
  pub select_mode: u8,
  pub units_buildings_number: u16,
  #[bin(repeat = "units_buildings_number")]
  pub selected_objects: Vec<ObjectPair>,
}

#[derive(Debug, BinDecode)]
pub struct ObjectPair {
  pub object_id_1: u32,
  pub object_id_2: u32,
}

#[derive(Debug, BinDecode)]
pub struct AssignGroupHotkey {
  pub group_number: u8,
  pub selected_object_number: u16,
  #[bin(repeat = "selected_object_number")]
  pub selected_objects: Vec<ObjectPair>,
}

#[derive(Debug, BinDecode)]
pub struct SelectGroupHotkey {
  pub group_number: u8,
  _unknown: u8,
}

#[derive(Debug, BinDecode)]
pub struct SelectSubgroup114b {
  pub item_id: u16,
  pub object: ObjectPair,
}

#[derive(Debug, BinDecode)]
pub struct SelectGroundItem {
  _unknown: u8,
  pub object: ObjectPair,
}

#[derive(Debug, BinDecode)]
pub struct CancelHeroRevival {
  pub object: ObjectPair,
}

#[derive(Debug, BinDecode)]
pub struct RemoveUnitFromBuildingQueue {
  pub slot_number: u8,
  pub item_id: u32,
}

#[derive(Debug, BinDecode)]
pub struct ChangeAllyOptions {
  pub player_slot_number: u8,
  pub flags: u32,
}

#[derive(Debug, BinDecode)]
pub struct TransferResources {
  pub player_slot_number: u8,
  pub gold_to_transfer: u32,
  pub lumber_to_transfer: u32,
}

#[derive(Debug, BinDecode)]
pub struct MapTriggerChatCommand {
  _unknown_a: u32,
  _unknown_b: u32,
  pub chat_command: CString,
}

#[derive(Debug, BinDecode)]
pub struct ScenarioTrigger {
  _unknown_a: u32,
  _unknown_b: u32,
  _unknown_counter: u32,
}

#[derive(Debug, BinDecode)]
pub struct MinimapSignal {
  pub location_x: u32,
  pub location_y: u32,
  _unknown: u32,
}

#[derive(Debug, BinDecode)]
pub struct MMDMessage {
  pub name: CString,
  pub checksum: CString,
  pub second_checksum: CString,
  pub weak_checksum: u32,
}

#[derive(Debug)]
pub struct Unknown<const SIZE: usize> {
  _unknown: [u8; SIZE],
}

impl<const SIZE: usize> BinDecode for Unknown<SIZE> {
  const MIN_SIZE: usize = SIZE;
  const FIXED_SIZE: bool = true;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(SIZE)?;
    let mut data = [0_u8; SIZE];
    buf.copy_to_slice(&mut data);
    Ok(Self { _unknown: data })
  }
}
