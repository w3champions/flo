use lazy_static::lazy_static;
use prometheus::{register_int_gauge, Encoder, IntGauge, TextEncoder};

use crate::error::*;
use hyper::header::CONTENT_TYPE;

lazy_static! {
  pub static ref PENDING_PLAYER_TOKENS: IntGauge = register_int_gauge!(
    "flonode_pending_player_tokens",
    "Number of pending player tokens"
  )
  .unwrap();
  pub static ref CONNECTED_PLAYERS: IntGauge =
    register_int_gauge!("flonode_connected_players", "Number of players connected").unwrap();
  pub static ref GAME_SESSIONS: IntGauge =
    register_int_gauge!("flonode_game_sessions", "Number of game sessions").unwrap();
}

pub async fn serve_metrics() -> Result<()> {
  use hyper::service::{make_service_fn, service_fn};
  use hyper::{Body, Request, Response, Server};
  use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

  async fn serve_req(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
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
