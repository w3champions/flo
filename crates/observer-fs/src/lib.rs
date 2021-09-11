use crate::error::{Error, Result};
use bytes::{Buf, BufMut, BytesMut};
use flate2::read::GzDecoder;
use flo_observer::record::GameRecordData;
use flo_util::binary::{BinDecode, BinEncode};
use flo_util::{BinDecode, BinEncode};
use once_cell::sync::Lazy;
use std::io::{Cursor, Read, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod error;

const MAX_CHUNK_SIZE: usize = 16 * 1024;
const CHUNK_PREFIX: &'static str = "chunk_";
const CHUNK_TEMP_FILENAME: &'static str = "_chunk";
const STATE_TEMP_FILENAME: &'static str = "_state";
const ARCHIVE_TEMP_FILENAME: &'static str = "_archive";
const ARCHIVE_FILENAME: &'static str = "archive.gz";
static DATA_FOLDER: Lazy<PathBuf> = Lazy::new(|| {
  let path = PathBuf::from("./data");
  std::fs::create_dir_all(&path).expect("create data folder");
  path
});

#[derive(Debug)]
pub struct GameDataWriter {
  game_id: i32,
  next_record_id: u32,
  chunk_id: usize,
  chunk_buf: BytesMut,
  dir: PathBuf,
  chunk_file: Option<File>,
}

impl GameDataWriter {
  pub const ARCHIVE_FILENAME: &'static str = ARCHIVE_FILENAME;
  pub fn data_folder() -> &'static Path {
    DATA_FOLDER.as_path()
  }

  pub fn data_dir(&self) -> &Path {
    self.dir.as_path()
  }

  pub async fn create_or_recover(game_id: i32) -> Result<Self> {
    let dir = DATA_FOLDER.join(game_id.to_string());
    let path = dir.join(CHUNK_TEMP_FILENAME);

    match fs::metadata(&path).await {
      Ok(_) => return Self::recover(game_id).await,
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
      Err(err) => return Err(err.into()),
    }

    fs::create_dir_all(&dir).await?;

    let chunk_file = File::create(path).await?;
    Ok(Self {
      game_id,
      next_record_id: 0,
      chunk_id: 0,
      chunk_buf: BytesMut::with_capacity(MAX_CHUNK_SIZE),
      dir,
      chunk_file: chunk_file.into(),
    })
  }

  pub async fn recover(game_id: i32) -> Result<Self> {
    let r = GameDataReader::open(game_id).await?;
    let path = r.dir.join(CHUNK_TEMP_FILENAME);
    let chunk_file = File::create(path).await?;
    Ok(Self {
      game_id,
      next_record_id: r.next_record_id,
      chunk_id: r.next_chunk_id,
      chunk_buf: r.chunk_buf,
      dir: r.dir,
      chunk_file: chunk_file.into(),
    })
  }

  pub fn next_record_id(&self) -> u32 {
    self.next_record_id
  }

  pub async fn write_record(&mut self, data: GameRecordData) -> Result<WriteRecordDestination> {
    if data.encode_len() > MAX_CHUNK_SIZE {
      tracing::warn!("over-sized record dropped: {:?}", data.type_id());
      self.next_record_id += 1;
      return Ok(WriteRecordDestination::CurrentChunk);
    }
    let mut r = WriteRecordDestination::CurrentChunk;
    if self.chunk_buf.len() + data.encode_len() > MAX_CHUNK_SIZE {
      self.flush_chunk().await?;
      r = WriteRecordDestination::NewChunk;
    }
    data.encode(&mut self.chunk_buf);
    self.next_record_id += 1;
    Ok(r)
  }

  pub async fn build_archive(&mut self, remove_chunks: bool) -> Result<PathBuf> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::prelude::*;
    self.flush_chunk().await?;
    let archive_temp_file_path = self.dir.join(ARCHIVE_TEMP_FILENAME);
    let archive_file_path = self.dir.join(ARCHIVE_FILENAME);
    tokio::task::block_in_place(|| {
      {
        let file = std::fs::File::create(&archive_temp_file_path)?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(&FileHeader::new(self.game_id).bytes())?;
        for i in 0..self.chunk_id {
          let mut chunk_file =
            std::fs::File::open(self.dir.join(format!("{}{}", CHUNK_PREFIX, i)))?;
          chunk_file.seek(SeekFrom::Start(4))?;
          std::io::copy(&mut chunk_file, &mut encoder)?;
        }
        encoder.flush()?;
      }

      std::fs::rename(archive_temp_file_path, &archive_file_path)?;

      if remove_chunks {
        for i in 0..self.chunk_id {
          std::fs::remove_file(self.dir.join(format!("{}{}", CHUNK_PREFIX, i))).ok();
        }
        std::fs::remove_file(self.dir.join(CHUNK_TEMP_FILENAME)).ok();
      }
      Ok(archive_file_path)
    })
  }

  pub async fn flush_state(&mut self) -> Result<()> {
    if self.chunk_buf.is_empty() {
      return Ok(());
    }

    let mut file = File::create(self.dir.join(STATE_TEMP_FILENAME)).await?;
    file.write_u32(self.next_record_id).await?;
    file.write_all(self.chunk_buf.as_ref()).await?;
    file.sync_all().await?;

    Ok(())
  }

  async fn flush_chunk(&mut self) -> Result<()> {
    if self.chunk_buf.is_empty() {
      return Ok(());
    }

    if let Err(err) = fs::remove_file(self.dir.join(STATE_TEMP_FILENAME)).await {
      match err.kind() {
        std::io::ErrorKind::NotFound => {}
        _ => return Err(err.into()),
      }
    }

    {
      let mut chunk_file = self.chunk_file.take().unwrap();
      chunk_file.write_u32(self.next_record_id).await?;
      chunk_file.write_all(self.chunk_buf.as_ref()).await?;
      self.chunk_buf.clear();
      chunk_file.sync_all().await?;
    }
    let name = format!("{}{}", CHUNK_PREFIX, self.chunk_id);
    fs::rename(self.dir.join(CHUNK_TEMP_FILENAME), self.dir.join(name)).await?;
    self.chunk_id += 1;
    let chunk_file = File::create(self.dir.join(CHUNK_TEMP_FILENAME)).await?;
    self.chunk_file = Some(chunk_file);
    Ok(())
  }
}

pub enum WriteRecordDestination {
  CurrentChunk,
  NewChunk,
}

pub struct GameDataReader {
  next_record_id: u32,
  next_chunk_id: usize,
  dir: PathBuf,
  chunk_buf: BytesMut,
}

impl GameDataReader {
  pub async fn open(game_id: i32) -> Result<Self> {
    let dir = DATA_FOLDER.join(game_id.to_string());
    let mut chunk_buf = BytesMut::with_capacity(MAX_CHUNK_SIZE);
    let mut stream = fs::read_dir(&dir).await?;
    let mut max_chunk_id: Option<usize> = None;
    let mut buffer_record_id = None;

    while let Some(entry) = stream.next_entry().await? {
      let file_name = entry.file_name();
      let name = if let Some(v) = file_name.to_str() {
        v
      } else {
        continue;
      };

      if name == CHUNK_TEMP_FILENAME {
        continue;
      }

      if name == STATE_TEMP_FILENAME {
        let mut content = Cursor::new(fs::read(entry.path()).await?);

        if content.remaining() < 4 {
          return Err(Error::InvalidBufferFile);
        }

        buffer_record_id = Some(content.get_u32());
        chunk_buf.put(content);
      }

      if entry.file_type().await?.is_file() && name.starts_with(CHUNK_PREFIX) {
        if name.starts_with(CHUNK_PREFIX) {
          let number = (&name[(CHUNK_PREFIX.len())..]).parse::<usize>();
          if let Ok(number) = number {
            max_chunk_id = max_chunk_id
              .map(|v| std::cmp::max(v, number))
              .or(Some(number));
          }
        }
      }
    }

    let next_record_id = {
      if let Some(id) = buffer_record_id {
        id
      } else {
        if let Some(id) = max_chunk_id.clone() {
          let mut last_chunk_file =
            fs::File::open(dir.join(format!("{}{}", CHUNK_PREFIX, id))).await?;
          last_chunk_file.read_u32().await?
        } else {
          0
        }
      }
    };

    Ok(Self {
      chunk_buf,
      next_record_id,
      next_chunk_id: max_chunk_id.map(|v| v + 1).unwrap_or(0),
      dir,
    })
  }

  pub fn records(self) -> GameDataReaderRecords {
    GameDataReaderRecords {
      inner: GameDataReaderRecordsInner::Chunks(self),
      current_chunk: None,
      chunk_buf: Cursor::new(vec![]),
    }
  }
}

pub struct GameDataArchiveReader {
  header: FileHeader,
  content: Vec<u8>,
}

impl GameDataArchiveReader {
  pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
    let content = fs::read(path).await?;
    let mut r = GzDecoder::new(Cursor::new(content));

    let mut header_buf: [u8; FileHeader::MIN_SIZE] = [0; FileHeader::MIN_SIZE];
    r.read_exact(&mut header_buf)?;
    let mut s = &header_buf as &[u8];
    let header = FileHeader::decode(&mut s).map_err(crate::error::Error::DecodeArchiveHeader)?;
    let content = tokio::task::block_in_place(|| -> Result<_> {
      let mut content = vec![];
      r.read_to_end(&mut content)?;
      Ok(content)
    })?;

    Ok(Self { header, content })
  }

  pub fn game_id(&self) -> i32 {
    self.header.game_id
  }

  pub fn records(self) -> GameDataReaderRecords {
    GameDataReaderRecords {
      inner: GameDataReaderRecordsInner::Content,
      current_chunk: Some(0),
      chunk_buf: Cursor::new(self.content),
    }
  }
}

pub struct GameDataReaderRecords {
  inner: GameDataReaderRecordsInner,
  current_chunk: Option<usize>,
  chunk_buf: Cursor<Vec<u8>>,
}

enum GameDataReaderRecordsInner {
  Content,
  Chunks(GameDataReader),
}

impl GameDataReaderRecordsInner {
  fn last_chunk_id(&self) -> usize {
    match *self {
      GameDataReaderRecordsInner::Content => 0,
      GameDataReaderRecordsInner::Chunks(ref inner) => inner.next_chunk_id - 1,
    }
  }
}

impl GameDataReaderRecords {
  pub async fn next(&mut self) -> Result<Option<GameRecordData>> {
    if !self.chunk_buf.has_remaining() {
      if !self.read_next_chunk().await? {
        return Ok(None);
      }
    }

    let record = GameRecordData::decode(&mut self.chunk_buf)?;
    Ok(Some(record))
  }

  pub async fn collect_vec(mut self) -> Result<Vec<GameRecordData>> {
    let mut all = vec![];
    while let Some(next) = self.next().await? {
      all.push(next);
    }
    Ok(all)
  }

  async fn read_next_chunk(&mut self) -> Result<bool> {
    if self.current_chunk == Some(self.inner.last_chunk_id()) {
      return Ok(false);
    }

    match self.inner {
      GameDataReaderRecordsInner::Content => unreachable!(),
      GameDataReaderRecordsInner::Chunks(ref mut inner) => {
        let id = self.current_chunk.map(|id| id + 1).unwrap_or(0);
        self.chunk_buf =
          Cursor::new(fs::read(inner.dir.join(format!("{}{}", CHUNK_PREFIX, id))).await?);
        if self.chunk_buf.remaining() < 4 {
          return Err(Error::InvalidChunkFile);
        }
        self.chunk_buf.advance(4);
        self.current_chunk = id.into();
      }
    }
    Ok(true)
  }
}

#[derive(Debug, BinEncode, BinDecode)]
struct FileHeader {
  #[bin(eq = FileHeader::SIGNATURE)]
  signature: [u8; 4],
  game_id: i32,
}

impl FileHeader {
  const SIGNATURE: &'static [u8] = b"flo\x01";

  fn new(game_id: i32) -> Self {
    let mut buf = [0; 4];
    buf.copy_from_slice(&Self::SIGNATURE);
    Self {
      signature: buf,
      game_id,
    }
  }

  fn bytes(&self) -> [u8; 8] {
    let mut buf = [0; 8];
    let mut s = &mut buf as &mut [u8];
    self.encode(&mut s);
    buf
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fs() {
  const N: usize = 10000;
  let game_id = i32::MAX;
  fs::remove_dir_all(DATA_FOLDER.join(game_id.to_string()))
    .await
    .ok();

  let records: Vec<_> = (0..N)
    .map(|id| GameRecordData::StopLag(id as i32))
    .collect();

  let mut writer = GameDataWriter::create_or_recover(game_id).await.unwrap();
  for record in records {
    writer.write_record(record).await.unwrap();
  }
  writer.build_archive(false).await.unwrap();

  fs::rename(
    writer.dir.join(ARCHIVE_FILENAME),
    writer.dir.join("_archive.gz"),
  )
  .await
  .unwrap();

  let mut writer = GameDataWriter::recover(game_id).await.unwrap();
  assert_eq!(writer.chunk_id, 4);
  writer.build_archive(false).await.unwrap();

  assert_eq!(
    fs::read(writer.dir.join("_archive.gz")).await.unwrap(),
    fs::read(writer.dir.join(ARCHIVE_FILENAME)).await.unwrap(),
  );

  // read

  fn validate_records(items: Vec<GameRecordData>) {
    for (i, r) in items.into_iter().enumerate() {
      let v = match r {
        GameRecordData::StopLag(id) => id as usize,
        _ => unreachable!(),
      };
      assert_eq!(i, v);
    }
  }

  let r = GameDataReader::open(game_id).await.unwrap();
  let records = r.records().collect_vec().await.unwrap();
  assert_eq!(records.len(), N);
  validate_records(records);

  let r = GameDataArchiveReader::open(writer.dir.join(ARCHIVE_FILENAME))
    .await
    .unwrap();
  let records = r.records().collect_vec().await.unwrap();
  assert_eq!(records.len(), N);
  validate_records(records);
}
