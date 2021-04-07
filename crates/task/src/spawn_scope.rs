use tokio::sync::watch::{channel, Receiver, Sender};

/// RAII guard used to notify child tasks that the parent has been dropped.
#[derive(Debug)]
pub struct SpawnScope {
  tx: Option<Sender<()>>,
  rx: Receiver<()>,
}

impl SpawnScope {
  pub fn new() -> Self {
    let (tx, rx) = channel(());
    Self { tx: Some(tx), rx }
  }

  pub fn handle(&self) -> SpawnScopeHandle {
    let rx = self.rx.clone();
    SpawnScopeHandle(rx)
  }

  pub fn close(&mut self) {
    self.tx.take();
  }
}

#[derive(Debug)]
pub struct SpawnScopeHandle(Receiver<()>);

impl Clone for SpawnScopeHandle {
  fn clone(&self) -> Self {
    let rx = self.0.clone();
    SpawnScopeHandle(rx)
  }
}

impl SpawnScopeHandle {
  pub async fn left(&mut self) {
    while self.0.changed().await.is_ok() {}
  }
}

#[tokio::test]
async fn test_initial_value() {
  use std::future::Future;
  use std::time::Duration;
  use tokio::time::sleep;
  let scope = SpawnScope::new();

  fn get_task(mut scope: SpawnScopeHandle) -> impl Future<Output = i32> {
    async move {
      let mut n = 0;
      loop {
        tokio::select! {
          _ = scope.left() => {
            return n
          }
          _ = sleep(Duration::from_millis(50)) => {
            n = n + 1
          }
        }
      }
    }
  }

  let t1 = tokio::spawn(get_task(scope.handle()));
  let t2 = tokio::spawn(get_task(scope.handle()));
  let t3 = tokio::spawn(get_task(scope.handle()));

  sleep(Duration::from_millis(100)).await;
  drop(scope);

  let (v1, v2, v3) = tokio::try_join!(t1, t2, t3).unwrap();
  assert!(v1 > 0);
  assert!(v2 > 0);
  assert!(v3 > 0);
}
