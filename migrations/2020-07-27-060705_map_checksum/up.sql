create table map_checksum (
    id serial not null primary key,
    sha1 text not null,
    checksum bytea not null,
    unique(sha1)
);

create index map_checksum_sha1 on map_checksum(sha1);