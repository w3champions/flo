use crate::grpc::get_grpc_client;
use crate::Result;
use flo_grpc::controller::*;
use flo_grpc::game::*;

const MAP: &str = r#"maps\frozenthrone\(4)twistedmeadows.w3x"#;

pub async fn create_game(players: Vec<i32>, ob: Option<i32>) -> Result<i32> {
  if players.is_empty() && ob.is_none() {
    panic!("Need to specify at least one player or observer");
  }

  let mut client = get_grpc_client().await;

  let nodes = client.list_nodes(()).await?.into_inner().nodes;
  let node_id = nodes.first().unwrap().id;

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
      map: Some(get_map_server()?),
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
      ..Default::default()
    })
    .await?;
  Ok(res.into_inner().game.unwrap().id)
}

pub fn get_map_server() -> Result<Map> {
  let map = Map {
    sha1: hex::decode("9524abb8e35ce7b158bfa4d4b8734234d6073ca5")?,
    checksum: 3851316688u32,
    name: "TEST".to_string(),
    description: "The Global Warming cannot be stopped and the last survivors turnout back to the upper Lands behind. Now, even the last dry lands are flooding and the last remainings are fighting for it.".to_string(),
    author: "OmGan, edit by ESL".to_string(),
    path: "maps/frozenthrone/community/(2)lastrefuge.w3x".to_string(),
    width: 84,
    height: 84,
    players: vec![
      MapPlayer { name: "Player 1".to_string(), r#type: 1, flags: 0, ..Default::default() },
      MapPlayer { name: "Player 2".to_string(), r#type: 1, flags: 0, ..Default::default() }
    ],
    forces: vec![
      MapForce { name: "Force 1".to_string(), flags: 0, player_set: 4294967295, ..Default::default() }
    ]
  };
  Ok(map)
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
