use crate::db::FileDB;
use crate::state::State;
use crate::torrent::TorrentManager;
use std::io;
use tokio::net::TcpListener;

pub struct Client {
    listener: TcpListener,
    state: State,
    torrents: Vec<TorrentManager>,
}

impl Client {
    // pub async fn new() -> anyhow::Result<Client> {
    //     let listener = connect_to_available_port(6881, 9).await?;
    //     let db = FileDB::open("./db.json".into());
    //     let state = State::new(db)?;
    //     let torrents = Vec::new();
    //     for (hash, metadata) in &state.data {
    //         let metadata = metadata.lock()?;
    //         if !metadata.finished {
    //
    //         }
    //     }
    //
    //     Ok(Client { listener, state })
    // }

    // pub async fn run(&self) -> anyhow::Result<()> {
    //     loop {
    //         let (stream, _) = listener.accept().await?;
    //         handle_stream(stream).await;
    //     }
    // }
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
