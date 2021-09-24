use crate::{archiver::ArchiverHandle, mem_cache::MemCacheMgr};
use flo_net::listener::FloListener;
use flo_state::{Actor, Handler, Message, async_trait, Owner};

const MAX_IN_MEM_GAME: usize = 300;

pub struct StreamServer {
  archiver: ArchiverHandle,
  mem_cache: Owner<MemCacheMgr<MAX_IN_MEM_GAME>>,
}