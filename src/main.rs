use bittorrent::Client;
use bittorrent::ipc::{Args, handle_cmd};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Some(cmd) => handle_cli_cmd(cmd),
        None => {
            let client = Client::new();
            client.run()
        }
    }
    Ok(())
}
