use crate::dot_torrent::DotTorrent;
use anyhow::Context;
use std::path::PathBuf;

pub async fn download_torrent(path_str: &str) -> anyhow::Result<String> {
    let mut path = PathBuf::from(path_str);
    path.set_extension("torrent");
    let dot_torrent = DotTorrent::read(path).await?;
    let files = dot_torrent.download_all().await?;
    let output = dot_torrent.info.name;
    tokio::fs::write(
        output,
        files.into_iter().next().expect("always one file").bytes(),
    )
    .await
    .context("failed to write `.torrent` file");
    Ok(format!("downloaded torrent from path: {path_str}"))
}
