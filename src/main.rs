mod cli;
use clap::Parser;
use cli::Cli;
mod client;

fn main() {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    rt.block_on(async {
        cli.run().await;
    });
}
