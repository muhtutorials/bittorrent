use bittorrent::Client;
use bittorrent::ipc::{Args, handle_cli_cmd};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        None => {
            let client = Client::new().await?;
            client.run();
        }
        Some(cmd) => handle_cli_cmd(cmd).await?,
    }
    Ok(())
}
