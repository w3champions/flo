use crate::grpc::get_grpc_client;
use crate::Result;
use flo_grpc::controller::*;
use flo_grpc::game::*;

const MAP: &str = r#"maps\frozenthrone\(10)ragingstream.w3x"#;

pub async fn create_game(players: Vec<i32>, ob: Option<i32>, node_id: Option<i32>) -> Result<i32> {
  if players.is_empty() && ob.is_none() {
    panic!("Need to specify at least one player or observer");
  }

  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = node_id.unwrap_or(nodes.first().unwrap().id);

  tracing::info!(node_id);

  let game_name = format!("GAME-{:x}", rand::random::<u32>());
  tracing::info!("game name = {}", game_name);

  let (player1_slot_settings, player1_id) = if players.len() > 0 {
    (
      SlotSettings {
        team: 0,
        color: 1,
        handicap: 100,
        status: 2,
        race: 4,
        ..Default::default()
      },
      Some(players[0]),
    )
  } else {
    (
      SlotSettings {
        team: 0,
        color: 1,
        computer: 2,
        handicap: 100,
        status: 2,
        race: 4,
        ..Default::default()
      },
      None,
    )
  };

  let (player2_slot_settings, player2_id) = if players.len() > 1 {
    (
      SlotSettings {
        team: 1,
        color: 2,
        handicap: 100,
        status: 2,
        race: 4,
        ..Default::default()
      },
      Some(players[1]),
    )
  } else {
    (
      SlotSettings {
        team: 1,
        color: 2,
        computer: 2,
        handicap: 100,
        status: 2,
        race: 4,
        ..Default::default()
      },
      None,
    )
  };

  let mut slots = vec![
    CreateGameSlot {
      player_id: player1_id,
      settings: Some(player1_slot_settings),
      ..Default::default()
    },
    CreateGameSlot {
      player_id: player2_id,
      settings: Some(player2_slot_settings),
      ..Default::default()
    },
  ];

  if let Some(id) = ob {
    slots.push(CreateGameSlot {
      player_id: Some(id),
      settings: SlotSettings {
        team: 24,
        color: 0,
        status: 2,
        handicap: 100,
        ..Default::default()
      }
      .into(),
    });
  }

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: game_name,
      map: Some(get_map()?),
      node_id,
      slots,
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

pub async fn create_2v2_game(players: Vec<i32>) -> Result<i32> {
  if players.len() != 4 {
    panic!("Need to specify 4 player ids");
  }

  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

  tracing::info!(node_id);

  let game_name = format!("GAME-{:x}", rand::random::<u32>());
  tracing::info!("game name = {}", game_name);

  let slots = players
    .into_iter()
    .enumerate()
    .map(|(i, player_id)| CreateGameSlot {
      player_id: Some(player_id),
      settings: Some(SlotSettings {
        team: if i < 2 { 1 } else { 2 },
        color: i as i32,
        computer: 2,
        handicap: 100,
        status: 2,
        race: 0,
        ..Default::default()
      }),
      ..Default::default()
    })
    .collect();

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: game_name,
      map: Some(get_map()?),
      node_id,
      slots,
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

pub async fn create_ffa_game(players: Vec<i32>) -> Result<i32> {
  if players.len() != 4 {
    panic!("Need to specify 4 player ids");
  }

  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

  tracing::info!(node_id);

  let game_name = format!("GAME-{:x}", rand::random::<u32>());
  tracing::info!("game name = {}", game_name);

  let slots = players
    .into_iter()
    .enumerate()
    .map(|(i, player_id)| CreateGameSlot {
      player_id: Some(player_id),
      settings: Some(SlotSettings {
        team: i as _,
        color: i as i32,
        computer: 2,
        handicap: 100,
        status: 2,
        race: 0,
        ..Default::default()
      }),
      ..Default::default()
    })
    .collect();

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: game_name,
      map: Some(get_map()?),
      node_id,
      slots,
      mask_player_names: Some(true),
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

pub async fn create_4v4_game(players: Vec<i32>) -> Result<i32> {
  if players.len() != 8 {
    panic!("Need to specify 8 player ids");
  }

  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

  tracing::info!(node_id);

  let game_name = format!("GAME-{:x}", rand::random::<u32>());
  tracing::info!("game name = {}", game_name);

  let slots = players
    .into_iter()
    .enumerate()
    .map(|(i, player_id)| CreateGameSlot {
      player_id: Some(player_id),
      settings: Some(SlotSettings {
        team: if i < 4 { 1 } else { 2 },
        color: i as i32,
        computer: 2,
        handicap: 100,
        status: 2,
        race: 0,
        ..Default::default()
      }),
      ..Default::default()
    })
    .collect();

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: game_name,
      map: Some(get_map()?),
      node_id,
      slots,
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

pub async fn create_rpg_game(players: Vec<i32>, ob: Option<i32>) -> Result<i32> {
  let mut client = get_grpc_client().await;

  assert_eq!(players.len(), 2);

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

  tracing::info!(node_id);

  let game_name = format!("GAME-{:x}", rand::random::<u32>());
  tracing::info!("game name = {}", game_name);

  let slots = (0..24)
    .map(|i| match i {
      0 => CreateGameSlot {
        player_id: Some(players[0]),
        settings: Some(SlotSettings {
          team: 0,
          color: i as i32,
          computer: 2,
          handicap: 100,
          status: 2,
          race: 0,
          ..Default::default()
        }),
        ..Default::default()
      },
      1 => CreateGameSlot {
        player_id: Some(players[1]),
        settings: Some(SlotSettings {
          team: 1,
          color: i as i32,
          computer: 2,
          handicap: 100,
          status: 2,
          race: 0,
          ..Default::default()
        }),
        ..Default::default()
      },
      8 | 11 | 12 | 13 => CreateGameSlot {
        settings: Some(SlotSettings {
          team: 0,
          color: i as i32,
          computer: 2,
          handicap: 100,
          status: 2,
          race: 0,
          ..Default::default()
        }),
        ..Default::default()
      },
      9 | 10 | 14 | 15 => CreateGameSlot {
        settings: Some(SlotSettings {
          team: 1,
          color: i as i32,
          computer: 2,
          handicap: 100,
          status: 2,
          race: 0,
          ..Default::default()
        }),
        ..Default::default()
      },
      23 => {
        if let Some(id) = ob {
          CreateGameSlot {
            player_id: Some(id),
            settings: SlotSettings {
              team: 24,
              color: 0,
              status: 2,
              handicap: 100,
              ..Default::default()
            }
            .into(),
          }
        } else {
          CreateGameSlot {
            settings: Some(Default::default()),
            ..Default::default()
          }
        }
      }
      _ => CreateGameSlot {
        settings: Some(Default::default()),
        ..Default::default()
      },
    })
    .collect();

  let res = client
    .create_game_as_bot(CreateGameAsBotRequest {
      name: game_name,
      map: Some(get_rpg_map()?),
      node_id,
      slots,
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
    checksum: checksum.xoro,
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

fn get_rpg_map() -> Result<Map> {
  let path = "maps/W3Champions/Legion_TD_7.0b_Team_OZE_W3C.w3x";
  let storage = flo_w3storage::W3Storage::from_env()?;
  let (map, checksum) = flo_w3map::W3Map::open_storage_with_checksum(&storage, path)?;
  dbg!(&map);
  let map = Map {
    sha1: checksum.sha1.to_vec(),
    checksum: checksum.xoro,
    name: "FLO_CLI".to_string(),
    description: map.description().to_string(),
    author: map.author().to_string(),
    path: path.to_string(),
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
