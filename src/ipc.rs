use {
    crate::cmd::{create_torrent, download_torrent},
    anyhow::bail,
    clap::{Parser, Subcommand},
    interprocess::local_socket::{
        GenericFilePath, GenericNamespaced, ListenerOptions,
        tokio::{Stream, prelude::*},
    },
    std::io,
    tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};

const SOCKET_NAME: &str = "torrent";

#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
#[clap(rename_all = "snake_case")]
pub enum Command {
    Download { path: String },
    Create { path: String },
    Test,
}

pub(crate) async fn ipc_server() -> anyhow::Result<()> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(name).create_tokio() {
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            // When a program that uses a file-type socket name terminates
            // its socket server without deleting the file, a "corpse socket"
            // remains, which can neither be connected to nor reused by a new
            // listener. Normally, Interprocess takes care of this on affected
            // platforms by deleting the socket file when the listener is
            // dropped. (This is vulnerable to all sorts of races and thus can
            // be disabled.)
            //
            // In a real program, instead of leaving it up to the user
            // to perform cleanup, one would use the .try_overwrite(true)
            // listener option to try to replace the socket.
            eprintln!(
                "Error: could not start server because the socket file is \
                    occupied. Please check if {SOCKET_NAME} is in use by another \
                    process and try again."
            );
            return Err(e.into());
        }
        result => result?,
    };
    // This is a good place to inform clients that the server is ready.
    eprintln!("Server running at {SOCKET_NAME}");
    loop {
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("There was an error with an incoming connection: {e}");
                continue;
            }
        };
        // Spawn new parallel asynchronous tasks onto the Tokio runtime and
        // hand the connection over to them so that multiple clients could be
        // processed simultaneously in a lightweight fashion.
        tokio::spawn(async move {
            // The outer match processes errors that happen when we're
            // connecting to something. The inner if-let processes errors
            // that happen during the connection.
            if let Err(e) = handle_conn(conn).await {
                eprintln!("Error while handling connection: {e}");
            }
        });
    }
}

async fn handle_conn(conn: Stream) -> io::Result<()> {
    let mut receiver = BufReader::new(&conn);
    let mut sender = &conn;
    let mut cmd = String::with_capacity(128);
    receiver.read_line(&mut cmd).await?;
    let resp = match handle_ipc_cmd(&cmd).await {
        Ok(msg) => format!("{}\n", msg),
        Err(e) => format!("Error: {}\n", e),
    };
    sender.write_all(resp.as_bytes()).await?;
    // Avoid holding up resources.
    drop(conn);
    // read_line keeps the line feed at the end.
    print!("Client sent command: {cmd}");
    Ok(())
}

async fn handle_ipc_cmd(cmd: &str) -> anyhow::Result<String> {
    let args: Vec<&str> = cmd.split(" ").collect();
    if args.is_empty() {
        bail!("no command was provided");
    }
    match args[0] {
        "create" => {
            if args.len() < 2 {
                bail!("path argument wasn't provided");
            }
            create_torrent(args[1]).await
        }
        "download" => {
            if args.len() < 2 {
                bail!("path argument wasn't provided");
            }
            download_torrent(args[1]).await
        }
        "test" => Ok(String::from("test")),
        unknown => bail!("unknown command: {unknown}"),
    }
}

pub(crate) async fn handle_cli_cmd(cmd: Command) -> anyhow::Result<()> {
    let name = if GenericNamespaced::is_supported() {
        format!("{}.sock", SOCKET_NAME).to_ns_name::<GenericNamespaced>()?
    } else {
        format!("/tmp/{}.sock", SOCKET_NAME).to_fs_name::<GenericFilePath>()?
    };
    let mut buf = String::with_capacity(128);
    let conn = Stream::connect(name).await?;
    // Create a buffered reader that wraps the connection by reference
    // so we can receive a single line.
    let mut receiver = BufReader::new(&conn);
    // The "sender" will just be a shared reference to the connection,
    // allowing us to read and write concurrently. It is okay to ditch
    // this reference and re-borrow it at any time to satisfy the borrow
    // check.
    let mut sender = &conn;
    let line = match cmd {
        Command::Create { path } => format!("create {path}"),
        Command::Download { path } => format!("download {path}"),
        Command::Test => format!("test"),
    };
    sender.write_all(line.as_bytes()).await?;
    receiver.read_line(&mut buf).await?;
    // Avoid holding up resources.
    drop(receiver);
    drop(conn);
    // read_line keeps the line feed at the end.
    print!("Server answered: {buf}");
    Ok(())
}
