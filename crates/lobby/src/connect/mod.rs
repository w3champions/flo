use flo_net;

use crate::db::ExecutorRef;
use crate::error::Result;
use crate::state::StorageHandle;

mod state;
pub use state::NotificationSender;

pub async fn serve(db: ExecutorRef, state: StorageHandle) -> Result<()> {
  Ok(())
}
