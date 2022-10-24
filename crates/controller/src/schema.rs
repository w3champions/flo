// @generated automatically by Diesel CLI.

diesel::table! {
    api_client (id) {
        id -> Int4,
        name -> Text,
        secret_key -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
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
        mask_player_names -> Bool,
        game_version -> Nullable<Text>,
        enable_ping_equalizer -> Bool,
    }
}

diesel::table! {
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

diesel::table! {
    map_checksum (id) {
        id -> Int4,
        sha1 -> Text,
        checksum -> Bytea,
    }
}

diesel::table! {
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

diesel::table! {
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

diesel::table! {
    player_ban (id) {
        id -> Int4,
        player_id -> Int4,
        ban_type -> Int4,
        ban_expires_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    player_mute (id) {
        id -> Int4,
        player_id -> Int4,
        mute_player_id -> Int4,
        created_at -> Timestamptz,
    }
}

diesel::joinable!(game -> node (node_id));
diesel::joinable!(game -> player (created_by));
diesel::joinable!(game_used_slot -> game (game_id));
diesel::joinable!(game_used_slot -> player (player_id));
diesel::joinable!(player -> api_client (api_client_id));
diesel::joinable!(player_ban -> player (player_id));

diesel::allow_tables_to_appear_in_same_query!(
    api_client,
    game,
    game_used_slot,
    map_checksum,
    node,
    player,
    player_ban,
    player_mute,
);
