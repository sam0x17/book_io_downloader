use std::path::PathBuf;

use crate::client::*;
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

    #[arg(long, env = "BLOCKFROST_PROJECT_ID")]
    /// Specifies the BlockFrost API `project_id` that will be used to query Cardano. This is
    /// required and automatically loaded from the `BLOCKFROST_PROJECT_ID` env var.
    pub project_id: String,

    /// Specifies the number of cover images that should be downloaded for the specified
    /// `policy_id`. Defaults to 10.
    #[arg(short, long)]
    pub num_covers: Option<usize>,

    /// Overrides the default book.io valid covers API URL with the specified URL.
    ///
    /// Must be a GET endpoint that conforms to the JSON schema utilized by
    /// https://api.book.io/api/v0/collections, which is the default value.
    #[arg(short, long)]
    pub api_url: Option<String>,

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

impl Cli {
    /// Runs the CLI with the specified options
    pub async fn run(&self) {
        let num_covers = self.num_covers.unwrap_or(10);
        let mut client = Client::with_project_id(&self.project_id);
        if let Some(override_api_url) = &self.api_url {
            client.book_api_url = override_api_url.clone();
        }
        client.quiet = self.quiet;
        client.simulate_early_kill = self.simulate_kill;
        client.slow = self.slow;

        match client
            .download_covers_for_policy(self.policy_id.as_str(), num_covers, &self.output_dir)
            .await
        {
            Ok(completed) => todo!(),
            Err(err) => match err {
                DownloadCoversError::UpdateCollectionIds(_) => todo!(),
                DownloadCoversError::InvalidId => todo!(),
                DownloadCoversError::BlockFrost(_) => todo!(),
                DownloadCoversError::MetadataMissing { asset_id } => todo!(),
                DownloadCoversError::MetadataFilesMissing { asset_id } => todo!(),
                DownloadCoversError::MetadataFileInvalid { asset_id, message } => todo!(),
                DownloadCoversError::MetadataFilesEmpty { asset_id } => todo!(),
                DownloadCoversError::MetadataFilesMissingHighResImage { asset_id } => todo!(),
                DownloadCoversError::DownloadErrors(_) => todo!(),
            },
        }
    }
}
