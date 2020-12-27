create table player_mute (
    id serial not null primary key,
    player_id integer not null references player(id),
    mute_player_id integer not null references player(id),
    created_at timestamp with time zone default now() not null,
    unique(player_id, mute_player_id)
);

create index player_mute_player_id on player_mute(player_id);
create index player_mute_mute_player_id on player_mute(mute_player_id);