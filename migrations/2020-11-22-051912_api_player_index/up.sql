create index player_source on player(source);
create index player_api_realm on player(realm) where source = 2;