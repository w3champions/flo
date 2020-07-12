use diesel::prelude::*;

use crate::db::DbConn;
use crate::error::Result;
use crate::node::types::Node;
use crate::schema::node;

pub fn get_all_nodes(conn: &DbConn) -> Result<Vec<Node>> {
  use node::dsl;
  let nodes = node::table.order((dsl::location, dsl::name)).load(conn)?;
  Ok(nodes)
}
