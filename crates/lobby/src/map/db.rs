use diesel::prelude::*;
use s2_grpc_utils::S2ProtoUnpack;
use serde::Deserialize;

use crate::db::DbConn;
use crate::error::*;
use crate::schema::map_checksum;

pub fn search_checksum(conn: &DbConn, sha1: String) -> Result<Option<u32>> {
  use map_checksum::dsl;
  let value = map_checksum::table
    .filter(dsl::sha1.eq(sha1))
    .select(dsl::checksum)
    .first::<Vec<u8>>(conn)
    .optional()?
    .and_then(|bytes| {
      if bytes.len() == 4 {
        let mut b = [0_u8; 4];
        b.copy_from_slice(&bytes[0..4]);
        Some(u32::from_le_bytes(b))
      } else {
        None
      }
    });
  Ok(value)
}

#[derive(Debug, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::MapChecksumImportItem")]
pub struct ImportItem {
  pub sha1: String,
  pub checksum: u32,
}

pub fn import(conn: &DbConn, mut items: Vec<ImportItem>) -> Result<usize> {
  use diesel::pg::upsert::excluded;
  use map_checksum::dsl;

  items.sort_by_cached_key(|i| i.sha1.clone());
  items.dedup_by(|a, b| a.sha1 == b.sha1);

  let inserts: Vec<_> = items
    .iter()
    .map(|item| Insert {
      sha1: item.sha1.as_ref(),
      checksum: item.checksum.to_le_bytes().to_vec(),
    })
    .collect();

  diesel::insert_into(map_checksum::table)
    .values(inserts)
    .on_conflict(dsl::sha1)
    .do_update()
    .set(dsl::checksum.eq(excluded(dsl::checksum)))
    .execute(conn)
    .map_err(Into::into)
}

#[derive(Debug, Insertable)]
#[table_name = "map_checksum"]
struct Insert<'a> {
  sha1: &'a str,
  checksum: Vec<u8>,
}
