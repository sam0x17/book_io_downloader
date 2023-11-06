mod cli;
use clap::Parser;
use cli::Cli;
mod client;

fn main() {
    let cli = Cli::parse();
}
