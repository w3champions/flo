update game set node_id = null;
drop index game_node_id;
alter table game
    drop constraint game_node_id_fkey;
alter table game
    rename column node_id to node;
alter table game
    alter column node type jsonb using null;