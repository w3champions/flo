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
  use hyper::service::{make_service_fn, service_fn};
  use hyper::{Body, Request, Response, Server};
  use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

  async fn serve_req(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if req.uri().path() == "/version" {
      let response = Response::builder()
        .status(200)
        .body(Body::from(crate::version::FLO_NODE_VERSION_STRING))
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
      .body(Body::from(buffer))
      .unwrap();

    Ok(response)
  }

  let addr = SocketAddr::from(SocketAddrV4::new(
    Ipv4Addr::UNSPECIFIED,
    flo_constants::NODE_HTTP_PORT,
  ));

  let server = Server::bind(&addr).serve(make_service_fn(|_| async {
    Ok::<_, hyper::Error>(service_fn(serve_req))
  }));
  server.await?;

  Ok(())
}
