use std::{path::PathBuf, process::exit};

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
    /// Defaults to the current directory if not specified.
    ///
    /// If partial or complete downloads of any of the cover images about to be downloaded
    /// already exist in the specified directory, they will be resumed or skipped accordingly.
    pub output_dir: Option<PathBuf>,

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
    /// <https://api.book.io/api/v0/collections>, which is the default value.
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
        let policy_id = &self.policy_id;
        let output_dir = self
            .output_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("Failed to read current directory"));

        let path = output_dir.display().to_string();

        match client
            .download_covers_for_policy(self.policy_id.as_str(), num_covers, &output_dir)
            .await
        {
            Ok(completed) => {
                let num = completed.len();
                println!("Successfully downloaded {num}/{num_covers} covers for policy_id `{policy_id}` to `{path}`")
            }
            Err(err) => {
                if self.quiet {
                    exit(1)
                }
                let endpoint = client.book_api_url;
                match err {
                    DownloadCoversError::UpdateCollectionIds(
                        UpdateCollectionIdsError::Request(err),
                    ) => {
                        let msg = err.to_string();
                        eprintln!(
                            "An error occurred communicating with or processing the response \
                            from the valid collections endpoint `{endpoint}`: \"{msg}\"."
                        )
                    }
                    DownloadCoversError::InvalidId => {
                        eprintln!(
                            "The collection_id/policy_id `{policy_id}` was not found in the \
                            list of valid ids from {endpoint}."
                        )
                    }
                    DownloadCoversError::BlockFrost(err) => {
                        let msg = err.to_string();
                        eprintln!(
                            "An error occurred trying to access the collection_id/policy_id \
                            `{policy_id}` or associated information via BlockFrost: \"{msg}\"."
                        )
                    }
                    DownloadCoversError::MetadataMissing { asset_id } => {
                        eprintln!(
                            "The Cardano asset with id `{asset_id}` was found successfully, \
                            however it has no metadata."
                        )
                    }
                    DownloadCoversError::MetadataFilesMissing { asset_id } => {
                        eprintln!(
                            "The Cardano asset with id `{asset_id}` was found successfully, \
                            and it has metadata, but it is missing the required files key in \
                            that metadata."
                        )
                    }
                    DownloadCoversError::MetadataFileInvalid { asset_id, message } => {
                        eprintln!(
                            "The Cardano asset with id `{asset_id}` was found successfully, \
                            and it has metadata, but the metadata for one or more of the \
                            entries in its files array was invalid: \"{message}\"."
                        )
                    }
                    DownloadCoversError::MetadataFilesEmpty { asset_id } => {
                        eprintln!(
                            "The Cardano asset with id `{asset_id}` was found successfully, \
                            and it has metadata, but the `files` array was empty."
                        )
                    }
                    DownloadCoversError::MetadataFilesMissingHighResImage { asset_id } => {
                        eprintln!(
                            "The Cardano asset with id `{asset_id}` was found successfully, \
                            and it has the required `files` array, but one or more entries \
                            in the array was missing a high-resolution image asset."
                        )
                    }
                    DownloadCoversError::DownloadErrors(errs) => {
                        eprintln!("One or more downloads encountered errors:");
                        for err in errs {
                            let cid = err.cid;
                            match err.error {
                                DownloadErrorInner::IpfsError(err) => {
                                    let msg = err.to_string();
                                    eprintln!(
                                        "An error occurred communicating with or downloading \
                                        from the IPFS network for cid `{cid}`: \"{msg}\"."
                                    );
                                }
                                DownloadErrorInner::IoError(err) => {
                                    let msg = err.to_string();
                                    eprintln!(
                                        "An IO error occurred trying to write data associated \
                                        with cid `{cid}` to `{path}`: \"{msg}\"."
                                    );
                                }
                                DownloadErrorInner::CorruptDownload => {
                                    eprintln!(
                                        "The file that was downloaded for cid `{cid}` has failed \
                                        the integrity test and is of the wrong size."
                                    )
                                }
                            }
                        }
                    }
                }
                exit(1)
            }
        }
    }
}
