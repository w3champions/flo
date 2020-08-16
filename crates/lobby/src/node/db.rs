use diesel::prelude::*;

use crate::db::DbConn;
use crate::error::*;
use crate::node::types::{Node, NodeConnConfig};
use crate::schema::node;

pub fn get_all_nodes(conn: &DbConn) -> Result<Vec<Node>> {
  use node::dsl;
  let nodes = node::table
    .filter(dsl::disabled.eq(false))
    .order((dsl::location, dsl::name))
    .load(conn)?;
  Ok(nodes)
}

pub fn get_node(conn: &DbConn, node_id: i32) -> Result<Node> {
  node::table
    .find(node_id)
    .first::<Node>(conn)
    .optional()?
    .ok_or_else(|| Error::NodeNotFound)
    .map_err(Into::into)
}

pub fn get_node_conn_configs(conn: &DbConn) -> Result<Vec<NodeConnConfig>> {
  use node::dsl;
  let nodes = node::table
    .filter(dsl::disabled.eq(false))
    .select((dsl::id, dsl::ip_addr, dsl::secret))
    .load(conn)?;
  Ok(nodes)
}
