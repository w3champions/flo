alter table player
    drop constraint player_source_source_id_key,
    add column api_client_id integer references api_client(id),
    add constraint player_api_client_source_source_id_key unique (api_client_id, source, source_id);
update player set api_client_id = 1;
alter table player
    alter column api_client_id set not null;