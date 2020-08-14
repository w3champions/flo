use parking_lot::RwLock;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::error::*;
use crate::packet::{FloPacket, Frame};

#[derive(Debug)]
pub struct Registry {
  id_counter: AtomicU64,
  ttl: Duration,
  pending: Arc<RwLock<HashMap<u64, PendingRequest>>>,
}

impl Registry {
  pub fn new(ttl: Duration) -> Self {
    Registry {
      id_counter: AtomicU64::new(0),
      ttl,
      pending: Arc::new(RwLock::new(HashMap::new())),
    }
  }

  pub async fn request(&self, frame: Frame) -> Result<Response> {
    let (sender, receiver) = oneshot::channel();
    let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
    let req = PendingRequest { sender };
    self.pending.write().insert(id, req);
    tokio::spawn({
      let pending = self.pending.clone();
      async move {
        tokio::time::delay_for(self.ttl).await;
        let request = pending.write().remove(&id);
        if let Some(mut request) = request {
          request.sender.send(Response::Timeout).await.ok()
        }
      }
    });
    let res = receiver.await.map_err(|_| Error::Cancelled)?;
    Ok(res)
  }
}

#[derive(Debug)]
pub enum Response {
  Frame(Frame),
  Timeout,
}

#[derive(Debug)]
struct PendingRequest {
  sender: oneshot::Sender<Response>,
}
