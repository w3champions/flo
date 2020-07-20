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
        node -> Nullable<Jsonb>,
        is_private -> Bool,
        secret -> Nullable<Int4>,
        is_live -> Bool,
        max_players -> Int4,
        created_by -> Nullable<Int4>,
        started_at -> Nullable<Timestamptz>,
        ended_at -> Nullable<Timestamptz>,
        slots -> Jsonb,
        meta -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
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
    }
}

joinable!(game -> player (created_by));

allow_tables_to_appear_in_same_query!(
    api_client,
    game,
    node,
    player,
);
