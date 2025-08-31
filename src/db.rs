use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use serde::Deserialize;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

#[derive(Deserialize, Clone)]
struct Config {
    id: usize,
    checksum: [u8; 32],
}

#[derive(Clone)]
pub struct FileDB {
    config_path: PathBuf,
    config: Config,
    path: PathBuf,
    data: Vec<u8>,
}

impl FileDB {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let config_path = path
            .as_path()
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .map(|file_name| {
                let new_file_name = String::from("config_") + file_name;
                let mut config_path = path.clone();
                config_path.set_file_name(new_file_name);
                config_path
            })
            .ok_or(anyhow::anyhow!("could not create config file path"))?;

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
            config = Config { id: 0, checksum: [0; 32]};
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

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(buf);
        hasher.update(b"\n");
        let checksum = hasher.finalize().into();
        if self.config.checksum == checksum {
            return Ok(());
        }
        self.config.checksum = checksum;
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

    pub fn generate_id(&mut self) -> usize {
        self.config.id += 1;
        self.config.id
    }
}
