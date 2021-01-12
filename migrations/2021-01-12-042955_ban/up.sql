create table player_ban (
    id serial not null primary key,
    player_id integer not null references player(id),
    ban_type integer not null,
    ban_expires_at timestamp with time zone,
    created_at timestamp with time zone default now() not null,
    unique(player_id, ban_type)
);

create index player_ban_player_id on player_ban(player_id);