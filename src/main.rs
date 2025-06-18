use bittorrent::torrent::Torrent;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use bittorrent::create::create_torrent;

#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
#[clap(rename_all = "snake_case")]
pub enum Command {
    Download {
        torrent: PathBuf,
    },
    Create {
        path: PathBuf,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Download { mut torrent } => {
            torrent.set_extension("torrent");
            let torrent = Torrent::read(torrent).await?;
            let files = torrent.download_all().await?;
            let output = torrent.info.name;
            tokio::fs::write(output, files.into_iter().next().expect("always one file").bytes()).await?
        }
        Command::Create { path } => {
            create_torrent(path).await?
        }
    }
    Ok(())
}