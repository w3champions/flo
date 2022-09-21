use backoff::backoff::Backoff;
use bytes::{BufMut, Bytes, BytesMut};
use rusoto_core::{credential::StaticProvider, request::HttpClient};
use rusoto_s3::{GetObjectRequest, S3Client, S3};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::mpsc;

pub mod error;
use crate::error::{Error, Result};

pub struct Archiver {
  s3_bucket: String,
  s3_client: Arc<S3Client>,
  rx: mpsc::Receiver<Msg>,
}

pub struct ArchiverOptions {
  pub aws_s3_bucket: String,
  pub aws_access_key_id: String,
  pub aws_secret_access_key: String,
  pub aws_s3_region: String,
}

impl Archiver {
  pub fn new(opts: ArchiverOptions) -> Result<(Self, ArchiverHandle)> {
    let s3_bucket = opts.aws_s3_bucket;
    let s3_client = Arc::new({
      let provider = StaticProvider::new(
        opts.aws_access_key_id,
        opts.aws_secret_access_key,
        None,
        None,
      );
      let client = HttpClient::new().unwrap();
      let region = opts
        .aws_s3_region
        .parse()
        .map_err(|_| Error::InvalidS3Credentials("invalid env AWS_S3_REGION"))?;
      S3Client::new_with(client, provider, region)
    });

    let (tx, rx) = mpsc::channel(100);

    Ok((
      Self {
        s3_bucket: s3_bucket.clone(),
        s3_client: s3_client.clone(),
        rx,
      },
      ArchiverHandle {
        tx,
        s3_bucket,
        s3_client,
      },
    ))
  }

  pub async fn serve(self) {
    let Self {
      s3_bucket,
      s3_client,
      mut rx,
    } = self;

    loop {
      tokio::select! {
        msg = rx.recv() => {
          match msg {
            Some(Msg::AddArchive(archive)) => {
              Self::upload(&s3_bucket, s3_client.clone(), archive).await;
            },
            None => break,
          }
        }
      };
    }
  }

  async fn upload(
    bucket: &str,
    s3_client: Arc<S3Client>,
    ArchiveInfo { game_id, data, md5 }: ArchiveInfo,
  ) {
    use futures::stream;
    use rusoto_core::{ByteStream, RusotoError};
    use rusoto_s3::PutObjectRequest;

    let span = tracing::info_span!("upload", game_id);

    let mut backoff = backoff::ExponentialBackoff::default();
    let mut sleep_backoff = || match backoff.next_backoff() {
      Some(d) => tokio::time::sleep(d),
      None => tokio::time::sleep(backoff.max_interval),
    };

    loop {
      let req = PutObjectRequest {
        key: format!("{}", game_id),
        body: Some(ByteStream::new_with_size(
          stream::iter(Some(Ok(data.clone()))),
          data.len(),
        )),
        content_md5: Some(md5.clone()),
        bucket: bucket.to_string(),
        ..Default::default()
      };
      match s3_client.put_object(req).await {
        Ok(_) => {
          span.in_scope(|| {
            tracing::info!("uploaded: {} bytes", data.len());
          });
          break;
        }
        Err(RusotoError::HttpDispatch(err)) => {
          span.in_scope(|| {
            tracing::warn!("http: {}", err);
          });
          sleep_backoff().await;
        }
        Err(RusotoError::Unknown(err)) => {
          span.in_scope(|| {
            tracing::warn!("unknown: {:?}", err);
          });
          sleep_backoff().await;
        }
        Err(err) => {
          span.in_scope(|| {
            tracing::error!("upload: {:?}", err);
          });
          break;
        }
      }
    }
  }
}

#[derive(Clone)]
#[allow(unused)]
pub struct ArchiverHandle {
  tx: mpsc::Sender<Msg>,
  s3_bucket: String,
  s3_client: Arc<S3Client>,
}

impl ArchiverHandle {
  pub fn add_archive(&self, archive: ArchiveInfo) -> bool {
    self.tx.try_send(Msg::AddArchive(archive)).is_ok()
  }
}

#[derive(Debug)]
enum Msg {
  AddArchive(ArchiveInfo),
}

#[derive(Debug)]
pub struct ArchiveInfo {
  pub game_id: i32,
  pub data: Bytes,
  pub md5: String,
}

pub struct Md5Writer<W> {
  inner: W,
  md5: md5::Context,
}

impl<W> Md5Writer<W> {
  pub fn finish(self) -> (md5::Digest, W) {
    (self.md5.compute(), self.inner)
  }
}

impl<W: Write> Md5Writer<W> {
  /// Create a new Md5Writer.
  pub fn new(w: W) -> Md5Writer<W> {
    Md5Writer {
      inner: w,
      md5: md5::Context::new(),
    }
  }
}

impl<W: Write> Write for Md5Writer<W> {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    let amt = self.inner.write(buf)?;
    self.md5.consume(&buf[..amt]);
    Ok(amt)
  }

  fn flush(&mut self) -> std::io::Result<()> {
    self.inner.flush()
  }
}

pub struct Fetcher {
  s3_bucket: String,
  s3_client: Arc<S3Client>,
}

impl Fetcher {
  pub fn new(opts: ArchiverOptions) -> Result<Self> {
    let s3_bucket = opts.aws_s3_bucket;
    let s3_client = Arc::new({
      let provider = StaticProvider::new(
        opts.aws_access_key_id,
        opts.aws_secret_access_key,
        None,
        None,
      );
      let client = HttpClient::new().unwrap();
      let region = opts
        .aws_s3_region
        .parse()
        .map_err(|_| Error::InvalidS3Credentials("invalid env AWS_S3_REGION"))?;
      S3Client::new_with(client, provider, region)
    });

    Ok(Self {
      s3_bucket: s3_bucket.clone(),
      s3_client: s3_client.clone(),
    })
  }

  pub async fn fetch(&self, game_id: i32) -> Result<Bytes> {
    use futures::StreamExt;
    let res = self
      .s3_client
      .get_object(GetObjectRequest {
        bucket: self.s3_bucket.clone(),
        key: format!("{}", game_id),
        ..Default::default()
      })
      .await?;
    let mut chunks = if let Some(body) = res.body {
      body
        .collect::<Vec<Result<Bytes, _>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
    } else {
      return Ok(Bytes::new());
    };
    if chunks.len() == 1 {
      Ok(chunks.remove(0))
    } else {
      let mut buf = BytesMut::new();
      for chunk in chunks {
        buf.put(chunk);
      }
      Ok(buf.freeze())
    }
  }
}
