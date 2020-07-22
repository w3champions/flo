mod handler;
pub(crate) mod message;
mod stream;

use async_tungstenite::WebSocketStream;
use futures::future::{self, FutureExt};
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use http::{Request, Response};
use parking_lot::RwLock;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tracing::field::debug;

use crate::error::{Error, Result};
use crate::state::FloStateRef;
use crate::ws::handler::WsHandler;
pub use crate::ws::message::OutgoingMessage;
use crate::ws::message::{ClientInfo, ErrorMessage, IncomingMessage, War3Info};
pub use crate::ws::stream::WsSenderRef;
use crate::ws::stream::*;

#[derive(Debug)]
pub struct Ws {
  g: FloStateRef,
  handler: RwLock<Option<WsHandler>>,
}

impl Ws {
  pub async fn init(g: FloStateRef) -> Result<Self> {
    Ok(Self {
      g,
      handler: RwLock::new(None),
    })
  }

  pub async fn serve(&self) -> Result<()> {
    use async_tungstenite::tokio::accept_hdr_async;
    use async_tungstenite::tungstenite::Error as WsError;
    use tokio::net::TcpListener;

    let port = self.g.config.local_port;
    let mut listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;

    tracing::debug!("listen on {}", listener.local_addr()?);

    while let Some(stream) = listener.incoming().try_next().await? {
      let addr = stream.peer_addr()?;
      let stream = match accept_hdr_async(stream, check_origin).await {
        Ok(stream) => stream,
        Err(WsError::Http(_)) => continue,
        Err(e) => {
          tracing::error!("{}", e);
          return Err(e.into());
        }
      };
      {
        *self.handler.write() = Some(WsHandler::new(self.g.clone(), stream));
      }
    }

    Ok(())
  }
}

fn check_origin(
  req: &Request<()>,
  res: Response<()>,
) -> Result<Response<()>, Response<Option<String>>> {
  use http::StatusCode;
  let origin = req.headers().get(http::header::ORIGIN).ok_or_else(|| {
    Response::builder()
      .status(StatusCode::BAD_REQUEST)
      .body(None)
      .unwrap()
  })?;
  let origin = origin.to_str().map_err(|_| {
    Response::builder()
      .status(StatusCode::BAD_REQUEST)
      .body(None)
      .unwrap()
  })?;
  if !flo_constants::CONNECT_ORIGINS.contains(&origin) {
    return Err(
      Response::builder()
        .status(StatusCode::FORBIDDEN)
        .body(None)
        .unwrap(),
    );
  }
  Ok(res)
}
