use crate::error::{Error, Result};
use backoff::backoff::Backoff;
use bytes::Bytes;
use flo_observer_fs::GameDataWriter;
use rusoto_core::{credential::StaticProvider, request::HttpClient};
use rusoto_s3::{S3Client, S3};
use std::{env, io::ErrorKind, path::PathBuf, sync::Arc, time::SystemTime};
use tokio::sync::mpsc;

pub struct Archiver {
  data_dir: PathBuf,
  s3_bucket: String,
  s3_client: Arc<S3Client>,
  rx: mpsc::Receiver<Msg>,
}

impl Archiver {
  pub fn new(data_dir: PathBuf) -> Result<Option<(Self, ArchiverHandle)>> {
    let s3_bucket = if let Ok(value) = env::var("AWS_S3_BUCKET") {
      value
    } else {
      return Ok(None);
    };
    let s3_client = Arc::new({
      let provider = StaticProvider::new(
        env::var("AWS_ACCESS_KEY_ID")
          .map_err(|_| Error::InvalidS3Credentials("missing env AWS_ACCESS_KEY_ID"))?,
        env::var("AWS_SECRET_ACCESS_KEY")
          .map_err(|_| Error::InvalidS3Credentials("missing env AWS_SECRET_ACCESS_KEY"))?,
        None,
        None,
      );
      let client = HttpClient::new().unwrap();
      let region = env::var("AWS_S3_REGION")
        .map_err(|_| Error::InvalidS3Credentials("missing env AWS_SECRET_ACCESS_KEY"))?
        .parse()
        .map_err(|_| Error::InvalidS3Credentials("invalid env AWS_S3_REGION"))?;
      S3Client::new_with(client, provider, region)
    });

    let (tx, rx) = mpsc::channel(10);

    Ok((
      Self {
        data_dir,
        s3_bucket: s3_bucket.clone(),
        s3_client: s3_client.clone(),
        rx,
      },
      ArchiverHandle {
        tx,
        s3_bucket,
        s3_client
      },
    ).into())
  }

  pub async fn serve(self) {
    let Self {
      data_dir,
      s3_bucket,
      s3_client,
      mut rx,
    } = self;
    let (fs_tx, mut fs_rx) = mpsc::channel(1);

    tokio::spawn(
      FsScanner {
        root: data_dir,
        tx: fs_tx,
      }
      .start(),
    );

    let mut fs_scanning = true;
    loop {
      tokio::select! {
        msg = rx.recv() => {
          match msg {
            Some(Msg::AddFolder(path)) => {
              Self::upload_and_remove(&s3_bucket, s3_client.clone(), path).await;
            },
            None => break,
          }
        }
        fs = fs_rx.recv(), if fs_scanning => {
          if let Some(path) = fs {
            Self::upload_and_remove(&s3_bucket, s3_client.clone(), path).await;
          } else {
            fs_scanning = false;
          }
        }
      };
    }
  }

  async fn upload_and_remove(bucket: &str, s3_client: Arc<S3Client>, folder_path: PathBuf) {
    use futures::stream;
    use rusoto_core::{ByteStream, RusotoError};
    use rusoto_s3::PutObjectRequest;

    let key = match folder_path.file_name().map(|v| v.to_string_lossy()) {
      Some(v) => v.to_string(),
      None => {
        tracing::error!("skip upload: {:?}: invalid file name", folder_path);
        return;
      }
    };

    match tokio::fs::metadata(&folder_path).await {
      Ok(_) => {}
      Err(err) if err.kind() == ErrorKind::NotFound => {
        return;
      }
      Err(err) => {
        tracing::error!("skip upload: {:?}: {}", folder_path, err);
        return;
      }
    }

    let bytes = match tokio::fs::read(folder_path.join(GameDataWriter::ARCHIVE_FILENAME)).await {
      Ok(bytes) => Bytes::from(bytes),
      Err(err) => {
        tracing::error!("read archive: {:?}: {}", folder_path, err);
        return;
      }
    };
    let md5_value = base64::encode(&*md5::compute(&bytes));
    let mut backoff = backoff::ExponentialBackoff::default();
    let mut sleep_backoff = || match backoff.next_backoff() {
      Some(d) => tokio::time::sleep(d),
      None => tokio::time::sleep(backoff.max_interval),
    };

    loop {
      let req = PutObjectRequest {
        key: key.clone(),
        body: Some(ByteStream::new_with_size(
          stream::iter(Some(Ok(bytes.clone()))),
          bytes.len(),
        )),
        content_md5: Some(md5_value.clone()),
        bucket: bucket.to_string(),
        ..Default::default()
      };
      match s3_client.put_object(req).await {
        Ok(_) => break,
        Err(RusotoError::HttpDispatch(err)) => {
          tracing::warn!("http: {}", err);
          sleep_backoff().await;
        }
        Err(RusotoError::Unknown(err)) => {
          tracing::warn!("unknown: {:?}", err);
          sleep_backoff().await;
        }
        Err(err) => {
          tracing::error!("upload: {:?}: {:?}", folder_path, err);
          break;
        }
      }
    }

    tokio::fs::remove_dir_all(&folder_path)
      .await
      .map_err(|err| tracing::error!("clean up: {:?}: {}", folder_path, err))
      .ok();
  }
}

#[derive(Clone)]
pub struct ArchiverHandle {
  tx: mpsc::Sender<Msg>,
  s3_bucket: String,
  s3_client: Arc<S3Client>,
}

impl ArchiverHandle {
  pub async fn add_folder(&self, path: PathBuf) -> bool {
    self.tx.send(Msg::AddFolder(path)).await.is_ok()
  }

  pub async fn fetch(&self, game_id: i32) -> Result<Option<Vec<Bytes>>> {
    use rusoto_s3::GetObjectRequest;
    use futures::stream::StreamExt;
    use rusoto_core::RusotoError;
    use rusoto_s3::GetObjectError;
  
    let key = game_id.to_string();
  
    let req = GetObjectRequest {
      key: key.clone(),
      bucket: self.s3_bucket.clone(),
      ..Default::default()
    };
    let parts = match self.s3_client.get_object(req).await {
      Ok(res) => {
        if let Some(stream) = res.body {
          stream.collect::<Vec<_>>().await.into_iter().collect::<Result<Vec<_>, _>>()?
        } else {
          return Ok(None)
        }
      },
      Err(RusotoError::Service(GetObjectError::NoSuchKey(_))) => return Ok(None),
      Err(err) => return Err(err.into()),
    };
    
    Ok(Some(parts))
  }
}

#[derive(Debug)]
enum Msg {
  AddFolder(PathBuf),
  // FindArchive {
  //   game_id: i32,
  //   tx: oneshot::Sender<ArchiveInfo>,
  // },
}

#[derive(Debug)]
pub struct ArchiveInfo {}

struct FsScanner {
  root: PathBuf,
  tx: mpsc::Sender<PathBuf>,
}

impl FsScanner {
  async fn start(self) {
    use std::fs;
    let Self { root, tx } = self;
    let now = SystemTime::now();
    tokio::task::spawn_blocking(move || {
      let iter = match fs::read_dir(root) {
        Ok(v) => v,
        Err(err) => {
          tracing::error!("read dir: {}", err);
          return;
        }
      };
      let mut total_count = 0;
      let mut sent_count = 0;
      for entry in iter {
        total_count += 1;
        match entry {
          Ok(entry) => match entry.metadata() {
            Ok(meta) => {
              if meta.is_dir() {
                if meta.created().map(|v| v < now).unwrap_or_default() {
                  if fs::metadata(entry.path().join(GameDataWriter::ARCHIVE_FILENAME)).is_ok() {
                    if tx.blocking_send(entry.path().to_owned()).is_err() {
                      break;
                    } else {
                      sent_count += 1;
                    }
                  }
                }
              }
            }
            Err(err) => {
              tracing::warn!("metadata: {:?}: {}", entry.path(), err);
            }
          },
          Err(err) => {
            tracing::warn!("fs scanner: {}", err);
            break;
          }
        }
      }
      tracing::info!(
        "fs scanner completed: {} total, {} sent",
        total_count,
        sent_count
      )
    })
    .await
    .ok();
  }
}

#[derive(Debug)]
pub struct ArchiveCache {
  parts: Vec<Bytes>,
  finished: bool,
}
