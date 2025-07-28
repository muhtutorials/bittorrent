use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use bittorrent::client::Client;
use bittorrent::create::create_torrent;
use bittorrent::dot_torrent::DotTorrent;
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
    Download { path: PathBuf },
    Create { path: PathBuf },
    Test,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Download { mut path } => {
            path.set_extension("torrent");
            let dot_torrent = DotTorrent::read(path).await?;
            let files = dot_torrent.download_all().await?;
            let output = dot_torrent.info.name;
            tokio::fs::write(
                output,
                files.into_iter().next().expect("always one file").bytes(),
            )
            .await?
        }
        Command::Create { path } => create_torrent(path).await?,
        Command::Test => {
            let mut map = HashMap::new();
            map.insert("one", 1);
            map.insert("two", 2);
            map.insert("three", 3);
            map.insert("four", 4);
            map.insert("five", 5);
            for (k, v) in map {
                println!("key: {k}, value: {v}");
            }
        },
    }
    Ok(())
}
