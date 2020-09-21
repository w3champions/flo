use crate::error::*;
use flo_state::{async_trait, Actor, Addr, Handler, Message};

use std::marker::PhantomData;

pub struct GetActorEntry<S, K = i32>(K, PhantomData<S>);

impl<S, K> GetActorEntry<S, K> {
  pub fn key(&self) -> &K {
    &self.0
  }
}

impl<S, K> Message for GetActorEntry<S, K>
where
  S: Actor,
  K: Send + 'static,
{
  type Result = Option<Addr<S>>;
}

#[async_trait]
impl<Parent, Entry, K> ActorMapExt<Entry, K> for Addr<Parent>
where
  Parent: Actor + Handler<GetActorEntry<Entry, K>>,
  Entry: Actor,
  K: Send + 'static,
{
  async fn send_to<M, R>(&self, key: K, message: M) -> Result<R>
  where
    M: Message<Result = Result<R>>,
    R: Send + 'static,
    Entry: Handler<M>,
  {
    let addr = match self.send(GetActorEntry(key, PhantomData)).await {
      Ok(Some(v)) => v,
      Ok(None) => return Err(Error::ActorNotFound),
      Err(err) => return Err(err.into()),
    };

    addr
      .send(message)
      .await
      .map_err(Error::from)
      .and_then(std::convert::identity)
  }
}

#[async_trait]
pub trait ActorMapExt<S, K = i32> {
  async fn send_to<M, R>(&self, key: K, message: M) -> Result<R>
  where
    M: Message<Result = Result<R>>,
    R: Send + 'static,
    S: Handler<M>;
}
