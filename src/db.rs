use anyhow::Context;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use tokio::fs::OpenOptions;

pub trait DB: Read + Write {
}

#[derive(Clone)]
pub struct FileDB {
    path: PathBuf,
    data: Cursor<Vec<u8>>,
    checksum: [u8; 32],
}

impl FileDB {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .read(true)
            .open(&path)
            .await
            .context(format!("couldn't open `{}`", path.display()))?;
        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read(&mut file, &mut buf).await?;
        let checksum = Sha256::digest(&buf).into();
        Ok(FileDB {
            path,
            data: Cursor::new(buf),
            checksum,
        })
    }
}

impl Read for FileDB {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.data.read(buf)
    }
}

impl Write for FileDB {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.data.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.data.flush()
    }
}
