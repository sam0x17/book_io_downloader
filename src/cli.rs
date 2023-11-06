use std::path::PathBuf;

use clap::{ArgAction, Parser};

/// Implementation of the `bcid` CLI
#[derive(Parser)]
#[command(name = "Book.io Cover Image Downloader (BCID) CLI")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Specifies the Cardano `policy_id`/`collection_id` you would like to download cover
    /// images for.
    pub policy_id: String,

    /// Specifies the filesystem directory you would like to download cover images to.
    ///
    /// If partial or complete downloads of any of the cover images about to be downloaded
    /// already exist in the specified directory, they will be resumed or skipped accordingly.
    pub output_dir: PathBuf,

    /// Specifies the number of cover images that should be downloaded for the specified
    /// `policy_id`. Defaults to 10.
    #[arg(short, long)]
    pub num_covers: Option<usize>,

    /// If enabled, suppresses all progress bar terminal output.
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub quiet: bool,

    /// If enabled, simulates a slow network connection using manual sleeps for easier
    /// debugging and visual inspection of download progress bars.
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub slow: bool,

    /// If enabled, will kill each download at a random percent completion (progress bars will
    /// suggest 100% completion).
    ///
    /// This is useful for debugging/testing the idempotence of the CLI when followed up by a
    /// subsequent call without `--simulate-kill`.
    #[arg(long, action = ArgAction::SetTrue)]
    pub simulate_kill: bool,
}
