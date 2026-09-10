use crate::DotTorrent;
use crate::State;

// downloads a torrent from a `.torrent` file
pub(crate) async fn download_torrent(path: &str, state: State) -> anyhow::Result<String> {
    let dot_torrent = DotTorrent::read(path).await?;
    state.add_torrent(dot_torrent).await?;
    Ok(String::from("download started"))
}
