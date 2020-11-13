use crate::grpc::get_grpc_client;
use flo_grpc::controller::CreateGameRequest;
use flo_grpc::game::*;

const MAP: &str = r#"maps\frozenthrone\(4)twistedmeadows.w3x"#;

pub async fn create_game(host_player_id: i32) -> i32 {
  let mut client = get_grpc_client().await;
  let res = client
    .create_game(CreateGameRequest {
      player_id: host_player_id,
      name: "TEST".to_string(),
      map: Some(get_map()),
      ..Default::default()
    })
    .await
    .unwrap();
  res.into_inner().game.unwrap().id
}

fn get_map() -> Map {
  let storage = flo_w3storage::W3Storage::from_env().unwrap();
  let (map, checksum) = flo_w3map::W3Map::open_storage_with_checksum(&storage, MAP).unwrap();
  let map = Map {
    sha1: checksum.sha1.to_vec(),
    checksum: u32::from_le_bytes([0xED, 0xB9, 0xC9, 0x08]),
    name: "FLO_CLI".to_string(),
    description: map.description().to_string(),
    author: map.author().to_string(),
    path: MAP.to_string(),
    width: map.dimension().0,
    height: map.dimension().1,
    players: map
      .get_players()
      .into_iter()
      .map(|v| MapPlayer {
        name: v.name.to_string(),
        r#type: v.r#type,
        race: v.race,
        flags: v.flags,
      })
      .collect(),
    forces: map
      .get_forces()
      .into_iter()
      .map(|v| MapForce {
        name: v.name.to_string(),
        flags: v.flags,
        player_set: v.player_set,
      })
      .collect(),
  };
  map
}
