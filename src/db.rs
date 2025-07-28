use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

#[derive(Clone)]
pub struct FileDB {
    path: PathBuf,
    data: Vec<u8>,
    checksum: [u8; 32],
}

impl FileDB {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .open(&path)
            .await
            .context(format!("couldn't open `{}`", path.display()))?;
        let mut buf = Vec::new();
        file.read(&mut buf).await?;
        if buf.len() == 0 {
            buf.extend("{}\n".as_bytes());
        }
        let checksum = Sha256::digest(&buf).into();
        Ok(FileDB {
            path,
            data: buf,
            checksum,
        })
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(buf);
        hasher.update(b"\n");
        let checksum = hasher.finalize().into();
        if self.checksum == checksum {
            return Ok(());
        }
        self.checksum = checksum;
        let file = File::create(&self.path).await?;
        let mut writer = BufWriter::new(file);
        writer.write_all(buf).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        self.data.clear();
        self.data.extend(buf);
        Ok(())
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}
