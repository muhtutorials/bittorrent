use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct Config {
    id: usize,
    checksum: [u8; 32],
}

#[derive(Clone)]
pub(crate) struct FileDB {
    config_path: PathBuf,
    config: Config,
    path: PathBuf,
    data: Vec<u8>,
}

impl FileDB {
    // Open DB where `path` is path to DB containing file.
    pub(crate) async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let config_path = path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .map(|file_name| {
                let mut config_path = path.clone();
                config_path.set_file_name(format!("config_{}", file_name));
                config_path
            })
            .context("could not create config file path")?;
        let mut config_file = OpenOptions::new()
            .create(true)
            .read(true)
            .open(&config_path)
            .await
            .context(format!("couldn't open `{}`", config_path.display()))?;
        let mut buf = Vec::new();
        config_file.read(&mut buf).await?;
        let mut config;
        let mut checksum_unset = false;
        if buf.len() == 0 {
            config = Config {
                id: 0,
                checksum: [0; 32],
            };
            checksum_unset = true;
        } else {
            config = serde_json::from_slice(&buf)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .open(&path)
            .await
            .context(format!("couldn't open `{}`", path.display()))?;
        buf.clear();
        file.read(&mut buf).await?;
        if buf.len() == 0 {
            buf.extend("{}\n".as_bytes());
        }
        if checksum_unset {
            config.checksum = Sha256::digest(&buf).into();
        }
        Ok(FileDB {
            config_path,
            config,
            path,
            data: buf,
        })
    }

    pub(crate) async fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(buf);
        hasher.update(b"\n");
        let checksum = hasher.finalize().into();
        if self.config.checksum == checksum {
            return Ok(());
        }
        self.config.checksum = checksum;
        let config_file = File::create(&self.config_path).await?;
        let mut config_writer = BufWriter::new(config_file);
        let config_str = serde_json::to_string(&self.config)?;
        config_writer.write_all(config_str.as_bytes()).await?;
        config_writer.write_all(b"\n").await?;
        config_writer.flush().await?;
        let file = File::create(&self.path).await?;
        let mut writer = BufWriter::new(file);
        writer.write_all(buf).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        self.data.clear();
        self.data.extend(buf);
        Ok(())
    }

    pub(crate) fn id(&self) -> usize {
        self.config.id
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }
}
