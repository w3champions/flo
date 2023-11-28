use once_cell::sync::Lazy;
use prometheus::{register_int_gauge, Encoder, IntGauge, TextEncoder};

use crate::error::*;
use hyper::header::CONTENT_TYPE;

pub static GAME_SESSIONS: Lazy<IntGauge> =
  Lazy::new(|| register_int_gauge!("flonode_game_sessions", "Number of game sessions").unwrap());
pub static PLAYERS_CONNECTIONS: Lazy<IntGauge> = Lazy::new(|| {
  register_int_gauge!(
    "flonode_player_connections",
    "Number of players connections"
  )
  .unwrap()
});
pub static PLAYER_TOKENS: Lazy<IntGauge> = Lazy::new(|| {
  register_int_gauge!(
    "flonode_player_tokens",
    "Number of registered player tokens"
  )
  .unwrap()
});

pub async fn serve_metrics() -> Result<()> {
  use bytes::Bytes;
  use http_body_util::Full;
  use hyper::{body, service::service_fn, Request, Response};
  use hyper_util::rt::TokioExecutor;
  use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

  async fn serve_req(req: Request<body::Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    if req.uri().path() == "/version" {
      let response = Response::builder()
        .status(200)
        .body(Full::new(crate::version::FLO_NODE_VERSION_STRING.into()))
        .unwrap();

      return Ok(response);
    }

    let encoder = TextEncoder::new();

    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();

    let response = Response::builder()
      .status(200)
      .header(CONTENT_TYPE, encoder.format_type())
      .body(Full::new(buffer.into()))
      .unwrap();

    Ok(response)
  }

  let addr = SocketAddr::from(SocketAddrV4::new(
    Ipv4Addr::UNSPECIFIED,
    flo_constants::NODE_HTTP_PORT,
  ));

  let listener = tokio::net::TcpListener::bind(&addr).await?;

  loop {
    let (stream, _) = listener.accept().await?;

    let io = hyper_util::rt::TokioIo::new(stream);

    tokio::spawn(async move {
      // use `auto::Builder` is for supporting both HTTP/1 and HTTP/2 at the same time.
      if let Err(err) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, service_fn(serve_req))
        .await
      {
        tracing::error!("Error serving connection: {}", err);
      }
    });
  }
}
