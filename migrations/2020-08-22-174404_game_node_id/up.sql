update game set node = null;
alter table game
    rename column node to node_id;
alter table game
    alter column node_id type integer using null;
alter table game
    add constraint game_node_id_fkey
    foreign key (node_id)
    references node(id);
create index game_node_id on game(node_id) where node_id is not null;
