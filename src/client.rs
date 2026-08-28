use crate::db::FileDB;
use crate::ipc::ipc_server;
use crate::state::State;
use crate::torrent::{Metadata, Torrent};
use crate::tracker::tracker_queries_task;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use tokio::net::TcpListener;

pub struct Client {
    db: FileDB,
    state: State,
    listener: TcpListener,
}

impl Client {
    pub async fn new() -> anyhow::Result<Self> {
        let db = FileDB::open(PathBuf::from("db.json")).await?;
        let id = db.id();
        let metadata_list: Vec<Metadata> = serde_json::from_slice(db.data())?;
        let mut torrents = HashMap::with_capacity(metadata_list.len());
        for metadata in metadata_list {
            torrents.insert(metadata.info_hash, Torrent::from_metadata(metadata));
        }
        let state = State::new(id, torrents);
        let listener = connect_to_available_port(6881, 9).await?;
        Ok(Self {
            db,
            state,
            listener,
        })
    }

    pub fn run(&self) {
        tokio::spawn(tracker_queries_task(self.state.clone()));
        tokio::spawn(ipc_server(self.state.clone()));
        loop {
            // let (stream, _) = self.listener.accept().await?;
            // handle_stream(stream).await;
        }
    }
}

async fn connect_to_available_port(base_port: u16, max_attempts: u16) -> io::Result<TcpListener> {
    for i in 0..max_attempts {
        let port = base_port + i;
        match TcpListener::bind(format!("127, 0, 0, 1:{port}")).await {
            Ok(listener) => return Ok(listener),
            Err(_) if i == max_attempts - 1 => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    format!(
                        "No available ports in range {}-{}",
                        base_port,
                        base_port + max_attempts - 1
                    ),
                ));
            }
            Err(_) => continue,
        }
    }
    unreachable!("loop should always return early");
}
