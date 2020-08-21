create table game_used_slot (
    id serial not null primary key,
    game_id integer not null references game(id) on delete cascade,
    player_id integer references player(id) on delete cascade,
    slot_index integer not null,
    team integer not null,
    color integer not null,
    computer integer not null,
    handicap integer not null,
    status integer not null,
    race integer not null,
    client_status integer not null,
    node_token bytea,
    created_at timestamp with time zone default now() not null,
    updated_at timestamp with time zone default now() not null,
    unique(game_id, player_id),
    unique(game_id, slot_index)
);
SELECT diesel_manage_updated_at('game_used_slot');

create index game_used_slot_game_id on game_used_slot(game_id);
create index game_used_slot_player_id on game_used_slot(player_id);