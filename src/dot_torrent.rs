use crate::download::{Downloaded, all};
use anyhow::Context;
use hashes::Hashes;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotTorrent {
    // The URL of the tracker.
    pub announce: String,
    pub info: Info,
}

impl DotTorrent {
    pub fn info_hash(&self) -> anyhow::Result<[u8; 20]> {
        let bencoded_info = serde_bencode::to_bytes(&self.info).context("bencode info section")?;
        let mut hasher = Sha1::new();
        hasher.update(&bencoded_info);
        Ok(hasher.finalize().into())
    }

    pub async fn read(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dot_torrent = tokio::fs::read(path).await.context("open torrent file")?;
        let torrent: DotTorrent =
            serde_bencode::from_bytes(&dot_torrent).context("parse torrent file")?;
        Ok(torrent)
    }

    pub fn print_tree(&self) {
        println!("torrent tree:");
        match &self.info.key {
            Key::SingleFile { .. } => {
                println!("{}", &self.info.name);
            }
            Key::MultipleFiles { files } => {
                for file in files {
                    println!("{}", file.path.join(std::path::MAIN_SEPARATOR_STR));
                }
            }
        }
    }

    pub fn length(&self) -> usize {
        match &self.info.key {
            Key::SingleFile { length } => *length,
            Key::MultipleFiles { files } => files.iter().map(|file| file.length).sum(),
        }
    }

    pub async fn download_all(&self) -> anyhow::Result<Downloaded> {
        all(self).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    // The `name` key maps to a UTF-8 encoded string which is
    // the suggested name to save the file (or directory) as.
    // In the single file case, the `name` key is the name of a file,
    // in the multiple file case, it's the name of a directory.
    pub name: String,

    #[serde(rename = "piece length")]
    // `piece length` maps to the number of bytes in each piece
    // the file is split into. For the purposes of transfer,
    // files are split into fixed-size pieces which are all
    // the same length except for possibly the last one which
    // may be truncated. `piece length` is almost always
    // a power of two, most commonly 2^18 = 256K
    // (BitTorrent prior to version 3.2 uses 2^20 = 1M as default).
    pub piece_length: usize,

    // `pieces` maps to a string whose length is a multiple of 20.
    // It is to be subdivided into strings of length 20, each of
    // which is the SHA1 hash of the piece at the corresponding index.
    pub pieces: Hashes,

    #[serde(flatten)]
    pub key: Key,
}

// There is also a key length or a key files, but not both or neither.
// If length is present then the download represents a single file,
// otherwise it represents a set of files which go in a directory structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Key {
    SingleFile { length: usize },
    MultipleFiles { files: Vec<File> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub length: usize,
    pub path: Vec<String>,
}

pub mod hashes {
    use serde::de::{Error, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;

    #[derive(Debug, Clone)]
    pub struct Hashes(pub Vec<[u8; 20]>);

    impl Serialize for Hashes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let flattened_vec = self.0.concat();
            serializer.serialize_bytes(&flattened_vec)
        }
    }

    impl<'de> Deserialize<'de> for Hashes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(HashesVisitor)
        }
    }

    struct HashesVisitor;

    impl<'de> Visitor<'de> for HashesVisitor {
        type Value = Hashes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte string whose length is a multiple of 20")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if v.len() % 20 != 0 {
                return Err(E::custom(format!("length is {}", v.len())));
            }
            Ok(Hashes(
                v.chunks_exact(20)
                    .map(|slice_20| slice_20.try_into().expect("guaranteed to be of length 20"))
                    .collect(),
            ))
        }
    }
}
