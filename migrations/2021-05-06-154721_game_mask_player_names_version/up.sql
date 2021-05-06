alter table game
    add column mask_player_names boolean default false not null,
    add column game_version text;