use crate::ipc::ipc_server;
use crate::state::State;
use crate::torrent::Metadata;
use crate::tracker::tracker_queries_task;
use crate::{Torrent, db::FileDB};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::channel;

pub struct Client {
    db: FileDB,
    state: State,
    listener: TcpListener,
}

impl Client {
    pub async fn new() -> anyhow::Result<Self> {
        let db = FileDB::open(PathBuf::from("db.json")).await?;
        let id = db.id();
        let meta_list: Vec<Metadata> = serde_json::from_slice(db.data())?;
        let mut torrents = HashMap::with_capacity(meta_list.len());
        for meta in meta_list {
            // TODO: not sure about the size
            let (tx, rx) = channel(1024);
            let torrent = Torrent::new(meta, rx);
            torrents.insert(meta.info_hash, torrent);
        }
        let state = State::new(id, torrents);
        let listener = connect_to_available_port(6881, 9).await?;
        Ok(Self {
            db,
            state,
            listener,
        })
    }

    pub async fn run(&self) {
        tokio::spawn(tracker_queries_task(self.state.clone()));
        tokio::spawn(ipc_server(self.state.clone()));
        loop {
            let Ok((conn, addr)) = self.listener.accept().await else {
                continue;
            };
            self.handle_conn(conn, addr).await;
        }
    }

    async fn handle_conn(&self, conn: TcpStream, addr: SocketAddr) {}

    async fn download_torrents(&self) {
        let torrents = &self.state.get().await.torrents;
        for torrent in torrents.values() {
            // if !torrent.finished {
            //     tokio::spawn(async move {})
            // }
        }
    }
}

async fn connect_to_available_port(base_port: u16, max_attempts: u16) -> io::Result<TcpListener> {
    for i in 0..max_attempts {
        let port = base_port + i;
        match TcpListener::bind(format!("127.0.0.1:{port}")).await {
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
