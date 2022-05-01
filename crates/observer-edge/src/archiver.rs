use crate::env::ENV;
use crate::error::{Error, Result};
use backoff::backoff::Backoff;
use bytes::Bytes;
use rusoto_core::{credential::StaticProvider, request::HttpClient};
use rusoto_s3::{S3Client, S3};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct Archiver {
  s3_bucket: String,
  s3_client: Arc<S3Client>,
  rx: mpsc::Receiver<Msg>,
}

impl Archiver {
  pub fn new() -> Result<Option<(Self, ArchiverHandle)>> {
    let s3_bucket = if let Some(value) = ENV.aws_s3_bucket.clone() {
      value
    } else {
      return Ok(None);
    };
    let s3_client = Arc::new({
      let provider = StaticProvider::new(
        ENV
          .aws_access_key_id
          .clone()
          .ok_or_else(|| Error::InvalidS3Credentials("missing env AWS_ACCESS_KEY_ID"))?,
        ENV
          .aws_secret_access_key
          .clone()
          .ok_or_else(|| Error::InvalidS3Credentials("missing env AWS_SECRET_ACCESS_KEY"))?,
        None,
        None,
      );
      let client = HttpClient::new().unwrap();
      let region = ENV
        .aws_s3_region
        .clone()
        .ok_or_else(|| Error::InvalidS3Credentials("missing env AWS_SECRET_ACCESS_KEY"))?
        .parse()
        .map_err(|_| Error::InvalidS3Credentials("invalid env AWS_S3_REGION"))?;
      S3Client::new_with(client, provider, region)
    });

    let (tx, rx) = mpsc::channel(100);

    Ok(
      (
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
      )
        .into(),
    )
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
        key: format!("test_{}", game_id),
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
          break
        },
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

  #[allow(unused)]
  pub async fn fetch(&self, game_id: i32) -> Result<Option<Vec<Bytes>>> {
    use futures::stream::StreamExt;
    use rusoto_core::RusotoError;
    use rusoto_s3::GetObjectError;
    use rusoto_s3::GetObjectRequest;

    let key = game_id.to_string();

    let req = GetObjectRequest {
      key: key.clone(),
      bucket: self.s3_bucket.clone(),
      ..Default::default()
    };
    let parts = match self.s3_client.get_object(req).await {
      Ok(res) => {
        if let Some(stream) = res.body {
          stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
        } else {
          return Ok(None);
        }
      }
      Err(RusotoError::Service(GetObjectError::NoSuchKey(_))) => return Ok(None),
      Err(err) => return Err(err.into()),
    };

    Ok(Some(parts))
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

  /// Get the writer that is wrapped by this Md5Writer by reference.
  pub fn get_ref(&self) -> &W {
    &self.inner
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
