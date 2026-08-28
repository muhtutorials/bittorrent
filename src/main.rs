use bittorrent::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::new().await?;
    client.run();
    Ok(())
}
