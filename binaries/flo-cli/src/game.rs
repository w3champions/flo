use crate::grpc::get_grpc_client;
use crate::Result;
use flo_grpc::controller::*;
use flo_grpc::game::*;

const MAP: &str = r#"maps\frozenthrone\(4)twistedmeadows.w3x"#;

pub async fn create_game(players: Vec<i32>) -> Result<i32> {
  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

  tracing::info!(node_id);

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: "TEST".to_string(),
      map: Some(get_map()?),
      node_id,
      slots: players
        .into_iter()
        .enumerate()
        .map(|(idx, id)| CreateGameSlot {
          player_id: Some(id),
          settings: SlotSettings {
            team: idx as i32,
            color: idx as i32,
            status: 2,
            handicap: 100,
            ..Default::default()
          }
          .into(),
        })
        .collect(),
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

fn get_map() -> Result<Map> {
  let storage = flo_w3storage::W3Storage::from_env()?;
  let (map, checksum) = flo_w3map::W3Map::open_storage_with_checksum(&storage, MAP)?;
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
  Ok(map)
}
