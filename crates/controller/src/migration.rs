use crate::db::DbConn;
use crate::error::*;

embed_migrations!("../../migrations");

pub fn run(conn: &DbConn) -> Result<()> {
  embedded_migrations::run(conn)?;
  Ok(())
}
