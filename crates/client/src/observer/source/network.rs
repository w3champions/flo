use crate::error::{Error, Result};
use bytes::Buf;
use flo_net::{
  observer::{PacketObserverConnect, PacketObserverConnectAccept, PacketObserverConnectReject},
  stream::FloStream,
};
use flo_observer::record::GameRecordData;
use flo_types::observer::GameInfo;
use futures::Stream;
use s2_grpc_utils::S2ProtoUnpack;
use std::{
  pin::Pin,
  task::{Context, Poll},
};
use tokio::net::ToSocketAddrs;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct NetworkSource {
  rx: mpsc::UnboundedReceiver<Result<GameRecordData>>,
  ct: CancellationToken,
}

impl Drop for NetworkSource {
  fn drop(&mut self) {
    self.ct.cancel();
  }
}

impl NetworkSource {
  pub async fn connect<A: ToSocketAddrs>(addr: A, token: String) -> Result<(GameInfo, Self)> {
    let ct = CancellationToken::new();

    let mut transport = FloStream::connect(addr).await?;
    transport
      .send(PacketObserverConnect {
        version: Some(crate::version::FLO_VERSION.into()),
        token,
      })
      .await?;

    let reply = transport.recv_frame().await?;

    let game: GameInfo = flo_net::try_flo_packet! {
      reply => {
        p: PacketObserverConnectAccept => {
          tracing::debug!("observer server version: {:?}", p.version);
          GameInfo::unpack(p.game)?
        }
        p: PacketObserverConnectReject => {
          return Err(Error::ObserverConnectionRequestRejected(p.reason()).into())
        }
      }
    };

    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(
      Worker {
        transport,
        tx,
        ct: ct.clone(),
      }
      .run(),
    );

    Ok((game, Self { rx, ct }))
  }
}

struct Worker {
  transport: FloStream,
  tx: mpsc::UnboundedSender<Result<GameRecordData>>,
  ct: CancellationToken,
}

impl Worker {
  async fn run(mut self) {
    use flo_net::packet::FramePayload;
    let mut total_bytes = 0;

    'main: loop {
      tokio::select! {
        _ = self.ct.cancelled() => {
          break;
        }
        r = self.transport.recv_frame() => {
          use flo_net::packet::PacketTypeId;
          match r {
            Ok(mut frame) => {
              match frame.type_id {
                PacketTypeId::Ping => {
                  frame.type_id = PacketTypeId::Pong;
                  if let Err(err) = self.transport.send_frame(frame).await {
                    self.tx.send(Err(err.into())).ok();
                    break;
                  }
                },
                PacketTypeId::ObserverData => {
                  match frame.payload {
                      FramePayload::Bytes(mut bytes) => {
                        total_bytes += bytes.len();
                        while bytes.remaining() > 0 {
                          match GameRecordData::decode(&mut bytes) {
                            Ok(record) => {
                              if !self.tx.send(Ok(record)).is_ok() {
                                break 'main;
                              }
                            },
                            Err(err) => {
                              self.tx.send(Err(err.into())).ok();
                              break 'main;
                            },
                          }
                        }
                      },
                      _ => {
                        self.tx.send(Err(Error::InvalidObserverDataFrame)).ok();
                        break;
                      },
                  };
                },
                PacketTypeId::ObserverDataEnd => {
                  tracing::debug!("observer data stream ended: {} bytes", total_bytes);
                  break;
                },
                t => {
                  tracing::warn!("received unexpected frame type: {:?}", t)
                }
              }
            },
            Err(err) => {
              self.tx.send(Err(err.into())).ok();
              break;
            },
          }
        }
      }
    }
  }
}

impl Stream for NetworkSource {
  type Item = Result<GameRecordData>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.rx.poll_recv(cx)
  }
}
