table! {
    api_client (id) {
        id -> Int4,
        name -> Text,
        secret_key -> Text,
        created_at -> Timestamptz,
    }
}

table! {
    game (id) {
        id -> Int4,
        name -> Text,
        map_name -> Text,
        status -> Int4,
        node_id -> Nullable<Int4>,
        is_private -> Bool,
        secret -> Nullable<Int4>,
        is_live -> Bool,
        max_players -> Int4,
        created_by -> Int4,
        started_at -> Nullable<Timestamptz>,
        ended_at -> Nullable<Timestamptz>,
        meta -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        random_seed -> Int4,
        locked -> Bool,
    }
}

table! {
    game_used_slot (id) {
        id -> Int4,
        game_id -> Int4,
        player_id -> Nullable<Int4>,
        slot_index -> Int4,
        team -> Int4,
        color -> Int4,
        computer -> Int4,
        handicap -> Int4,
        status -> Int4,
        race -> Int4,
        client_status -> Int4,
        node_token -> Nullable<Bytea>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        client_status_synced_node_conn_id -> Nullable<Int8>,
    }
}

table! {
    map_checksum (id) {
        id -> Int4,
        sha1 -> Text,
        checksum -> Bytea,
    }
}

table! {
    node (id) {
        id -> Int4,
        name -> Text,
        location -> Text,
        secret -> Text,
        ip_addr -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        country_id -> Text,
        disabled -> Bool,
    }
}

table! {
    player (id) {
        id -> Int4,
        name -> Text,
        source -> Int4,
        source_id -> Text,
        source_state -> Nullable<Jsonb>,
        realm -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        api_client_id -> Int4,
    }
}

joinable!(game -> node (node_id));
joinable!(game -> player (created_by));
joinable!(game_used_slot -> game (game_id));
joinable!(game_used_slot -> player (player_id));
joinable!(player -> api_client (api_client_id));

allow_tables_to_appear_in_same_query!(
    api_client,
    game,
    game_used_slot,
    map_checksum,
    node,
    player,
);
