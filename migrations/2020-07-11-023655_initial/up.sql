create table player (
    id serial not null primary key,
    name text not null,
    source integer not null,
    source_id text not null,
    source_state jsonb,
    realm text,
    created_at timestamp with time zone default now() not null,
    updated_at timestamp with time zone default now() not null,
    unique(source, source_id)
);
SELECT diesel_manage_updated_at('player');

create index player_source_id on player(source, source_id);

create table game (
    id serial not null primary key,
    name text not null,
    map_name text not null,
    status integer not null default 0,
    node jsonb,
    is_private boolean not null,
    secret integer,
    is_live boolean not null,
    max_players integer not null,
    created_by integer references player(id),
    started_at timestamp with time zone,
    ended_at timestamp with time zone,
    meta jsonb not null,
    created_at timestamp with time zone default now() not null,
    updated_at timestamp with time zone default now() not null
);
SELECT diesel_manage_updated_at('game');

create index game_status on game(status);

create table node (
    id serial not null primary key,
    name text not null,
    location text not null,
    secret text not null,
    ip_addr text not null,
    created_at timestamp with time zone default now() not null,
    updated_at timestamp with time zone default now() not null
);
SELECT diesel_manage_updated_at('node');

create table api_client (
    id serial not null primary key,
    name text not null,
    secret_key text not null,
    created_at timestamp with time zone default now() not null
);