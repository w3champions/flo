alter table player
    drop constraint player_api_client_source_source_id_key,
    drop column api_client_id,
    add constraint player_source_source_id_key unique (source, source_id);