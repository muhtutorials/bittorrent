// bittorrent specs https://www.bittorrent.org/beps/bep_0003.html
use bittorrent::torrent::Torrent;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Download { torrent, output } => {
            let torrent = Torrent::read(torrent).await?;
            let files = torrent.download_all().await?;
            tokio::fs::write(output, files.into_iter().next().expect("always one file").bytes()).await?
        }
    }
    Ok(())
}