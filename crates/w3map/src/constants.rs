use bitflags::bitflags;

bitflags! {
  pub struct MapFlags: u32 {
    const HIDE_MINIMAP = 0x0001;
    const MODIFY_ALLY_PRIORITIES = 0x0002;
    const MELEE = 0x0004;
    const REVEAL_TERRAIN = 0x0010;
    const FIXED_PLAYER_SETTINGS = 0x0020;
    const CUSTOM_FORCES = 0x0040;
    const CUSTOM_TECH_TREE = 0x0080;
    const CUSTOM_ABILITIES = 0x0100;
    const CUSTOM_UPGRADES = 0x0200;
    const WATER_WAVES_ON_CLIFF_SHORES = 0x0800;
    const WATER_WAVES_ON_SLOPE_SHORES = 0x1000;
  }
}
