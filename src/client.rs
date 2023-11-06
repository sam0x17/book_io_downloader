//! Contains the [`Client`] used by the `bicd` CLI utility and supporting types.

use blockfrost::{BlockFrostApi, JsonValue};
use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use ipfs_api::IpfsClient;
use ipfs_api_backend_hyper::IpfsApi;
use rand::seq::SliceRandom;
use reqwest;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// This is the default API url that will be used to check if a book policy_id/collection_id is
/// valid.
///
/// This can be overridden by setting [`Client::book_api_url`] and must be set to a valid HTTP
/// GET endpoint that returns a list of book collections that match the formatting dictated by
/// [`CollectionResponse`] and [`Collection`].
pub const BOOK_API_URL: &'static str = "https://api.book.io/api/v0/collections";

/// Internal struct used to serialize/deserialize the top level response from
/// [`Client::book_api_url`].
#[derive(Deserialize, Debug)]
pub struct CollectionResponse {
    /// Should always be set to "success" if the request succeeded
    #[serde(rename = "type")]
    pub response_type: String,
    /// Contains the actual [`Collection`]s that correspond with valid `policy_id`/`collection_id`s
    pub data: Vec<Collection>,
}

/// Corresponds with an item within the [`CollectionResponse::data`] array.
///
/// Each [`Collection`] represents a collection of books on the specified
/// [`Collection::blockchain`] and [`Collection::network`], where the
/// [`Collection::collection_id`] is a valid `policy_id` in the specified blockchain/network
/// pair.
#[derive(Deserialize, Debug)]
pub struct Collection {
    /// A unique identifier for this collection that doubles as a `policy_id` or equivalent on
    /// the blockchain network where this collection exists.
    pub collection_id: String,
    #[allow(unused)]
    /// Provides a short human-readable description of the [`Collection`].
    pub description: String,
    /// The blockchain (i.e. `cardano`) where this collection is hosted. Note that only
    /// `cardano` is supported by this library.
    pub blockchain: String,
    /// The network (i.e. `mainnet`, `testnet`, etc) of the blockchain where this collection is
    /// hosted.
    pub network: String,
}

/// This is the main type that contains all functionality supported by the CLI. Anything the
/// CLI can do can also be done with this public interface.
#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    blockfrost_client: BlockFrostApi,
    valid_cids: HashSet<String>,
    /// Should be the URL of an HTTP GET endpoint conforming to the JSON schema dictated by
    /// [`CollectionResponse`].
    ///
    /// This API is used to determine whether a given `policy_id`/`collection_id` is valid
    /// before trying to find it on Cardano.
    pub book_api_url: String,
    /// If set to `true`, suppresses terminal-based progress bars and other output during file
    /// downloads. Defaults to `false`.
    pub quiet: bool,
    /// If set to `true`, artificial sleeps are introduced to intentionally slow down downloads
    /// to the point where it is easy to visually inspect the progress bars while
    /// [`Client::download_covers_for_policy`] is running. Defaults to `false`.
    pub slow: bool,
    /// If set to `true`, will terminate particular downloads at random completion percents
    /// when [`Client::download_covers_for_policy`] and/or [`Client::download_files`] is run.
    ///
    /// The downloads in question will act as if they are complete. Note that 100% and 0%
    /// completion are both possible with how the random numbers are generated (and this is
    /// intentional). This is useful for testing idempotence.
    pub simulate_early_kill: bool,
}

/// Encapsulates errors that can be returned for a particular download.
#[derive(Debug)]
pub enum DownloadErrorInner {
    /// An error occurred communicating with or downloading from the IPFS network.
    IpfsError(ipfs_api::Error),
    /// An IO error occurred trying to write to the specified download destination path.
    IoError(std::io::Error),
    /// The resulting downloaded file is of the wrong size.
    CorruptDownload,
}

impl From<ipfs_api::Error> for DownloadErrorInner {
    fn from(value: ipfs_api::Error) -> Self {
        DownloadErrorInner::IpfsError(value)
    }
}

impl From<std::io::Error> for DownloadErrorInner {
    fn from(value: std::io::Error) -> Self {
        DownloadErrorInner::IoError(value)
    }
}

/// Annotates [`DownloadErrorInner`] with a `cid` field allowing us to know what download this
/// error refers to.
#[derive(Debug)]
pub struct DownloadError {
    /// The id of the cover for which this specific download failed.
    pub cid: String,
    /// The error that occurred while trying to download the cover.
    pub error: DownloadErrorInner,
}

impl DownloadError {
    /// Allows for easy construction of a [`DownloadError`] from a `cid` and something
    /// compatible with [`DownloadErrorInner`].
    pub fn from<C: AsRef<str>, E: Into<DownloadErrorInner>>(cid: C, error: E) -> Self {
        DownloadError {
            cid: cid.as_ref().to_owned(),
            error: error.into(),
        }
    }
}

/// Encapsulates all errors that can be thrown while trying to update the list of valid
/// collection ids from [`Client::book_api_url`].
#[derive(Debug)]
pub enum UpdateCollectionIdsError {
    /// A network issue was encountered or the response returned by the server indicates an
    /// error state or otherwise does not conform to the expected JSON schema.
    Request(reqwest::Error),
}

impl From<reqwest::Error> for UpdateCollectionIdsError {
    fn from(value: reqwest::Error) -> Self {
        UpdateCollectionIdsError::Request(value)
    }
}

/// Encapsulates all errors that can be encountered while
/// [`Client::download_covers_for_policy`] is running.
#[derive(Debug)]
pub enum DownloadCoversError {
    /// An error was encountered while attempting to update the list of valid collection ids.
    UpdateCollectionIds(UpdateCollectionIdsError),
    /// The provided `collection_id`/`policy_id` was not found in the list of valid ids.
    InvalidId,
    /// An error occurred trying to access the specified `collection_id/policy_id` or
    /// associated information via BlockFrost.
    BlockFrost(blockfrost::Error),
    /// The specified asset was found successfully on the blockchain,
    /// however it has no metadata.
    MetadataMissing {
        /// The ID of the asset
        asset_id: String,
    },
    /// The specified asset was found successfully on the blockchain, and
    /// it has metadata, but it is missing the required `files` key in that metadata.
    MetadataFilesMissing {
        /// The ID of the asset
        asset_id: String,
    },
    /// The specified asset was found successfully on the blockchain, but
    /// the metadata for one or more of the entries in its `files` array was invalid.
    MetadataFileInvalid {
        /// The ID of the asset
        asset_id: String,
        /// A helpful message describing the error state
        message: &'static str,
    },
    /// The specified asset was found successfully on the blockchain, but its `files` array was empty.
    MetadataFilesEmpty {
        /// The ID of the asset
        asset_id: String,
    },
    /// The specified asset was found successfully on the blockchain, but one or more files in
    /// its `files` array is missing a high-res image.
    MetadataFilesMissingHighResImage {
        /// The id of the asset
        asset_id: String,
    },
    /// The download process is complete however one or more errors was encountered during
    /// download. See the [`Vec`] for the list of specific errors.
    DownloadErrors(Vec<DownloadError>),
}

impl From<UpdateCollectionIdsError> for DownloadCoversError {
    fn from(value: UpdateCollectionIdsError) -> Self {
        DownloadCoversError::UpdateCollectionIds(value)
    }
}

impl From<blockfrost::Error> for DownloadCoversError {
    fn from(value: blockfrost::Error) -> Self {
        DownloadCoversError::BlockFrost(value)
    }
}

impl From<DownloadError> for DownloadCoversError {
    fn from(value: DownloadError) -> Self {
        DownloadCoversError::DownloadErrors(vec![value])
    }
}

/// Encapsulates all errors that can be thrown when creating a new instance of [`Client`].
#[derive(Debug, PartialEq, Eq)]
pub enum CreateClientError {
    /// A BlockFrost project ID has not been specified. This can either be provided directly
    /// using [`Client::with_project_id`] or using the `BLOCKFROST_PROJECT_ID` environment
    /// variable.
    MissingProjectId,
}

impl Client {
    /// Creates a new [`Client`] using default values for all fields.
    ///
    /// Requires a valid BlockFrost project id to be specified in the `BLOCKFROST_PROJECT_ID`
    /// environment variable, or an error will be returned.
    pub fn new() -> Result<Self, CreateClientError> {
        let project_id =
            std::env::var("BLOCKFROST_PROJECT_ID").or(Err(CreateClientError::MissingProjectId))?;
        Ok(Client::with_project_id(&project_id))
    }

    /// Creates a new [`Client`] using default values for all fields and the specified
    /// `blockfrost_project_id` which must be a valid project ID for use with the BlockFrost API.
    ///
    /// Note that you _should not_ commit your `blockfrost_project_id` to version control!
    pub fn with_project_id(blockfrost_project_id: &str) -> Self {
        // note that BlockFrost claims that their `new` method will panic if there is an
        // invalid project ID. I have tested this and it is actually incorrect, instead you see
        // this later as an error when you go to do something with the `BlockFrostApi` object,
        // so that is why this method is infallible.
        Client {
            ipfs_client: IpfsClient::default(),
            blockfrost_client: BlockFrostApi::new(blockfrost_project_id, Default::default()),
            book_api_url: BOOK_API_URL.to_string(),
            valid_cids: HashSet::new(),
            quiet: false,
            slow: false,
            simulate_early_kill: false,
        }
    }

    /// Provides an easy means for updating the cached set of valid
    /// `policy_id`/`collection_id`s.
    ///
    /// This will overwrite the existing set if called. Returns an [`UpdateCollectionIdsError`]
    /// in the event of an error.
    ///
    /// If the set is currently empty, this method is automatically called at the start of
    /// [`Client::download_covers_for_policy`] so there is no need to manually call this unless
    /// you suspect the cached set is stale.
    pub async fn update_collection_ids(&mut self) -> Result<(), UpdateCollectionIdsError> {
        let response = reqwest::get(&self.book_api_url).await?.error_for_status()?;
        let collection_response: CollectionResponse = response.json().await?;
        if collection_response.response_type != "success" {}
        self.valid_cids = collection_response
            .data
            .into_iter()
            .filter(|collection| {
                collection.blockchain == "cardano" // && collection.network == "mainnet"
            })
            .map(|collection| collection.collection_id)
            .collect();
        Ok(())
    }

    /// Downloads `num_covers` cover images associated with the specified `policy_id` to the
    /// specified `target_dir`.
    ///
    /// The `policy_id` is first checked against [`Client::book_api_url`] to see if it is a
    /// valid `collection_id` according to that API.
    ///
    /// If it passes this test, then we query the Cardano blockchain using BlockFrost to obtain
    /// `num_covers` assets associated with that `policy_id`, and we download the high
    /// resolution cover images associated with those assets to the specified `target_dir`.
    ///
    /// All downloads are performed in parallel and terminal-based progress bars are displayed
    /// for each active download while they are underway. This can be turned off by setting
    /// [`Client::quiet`] to `true`, but it is on by default.
    ///
    /// Files will be named using the format `[asset_id].[ext]` where `ext` is an appropriate
    /// extension for the underlying image format for that cover. The `png` extension will be
    /// used for unrecognized image formats. Supported formats are defined by the
    /// [`extension_for`] function.
    ///
    /// If partial or complete downloads of the same files already exist in the specified
    /// `target_dir`, they will be resumed and/or skipped accordingly, making this function
    /// idempotent.
    ///
    /// Various debugging options for this function can be controlled directly via fields on
    /// [`Client`].
    pub async fn download_covers_for_policy(
        &mut self,
        policy_id: &str,
        num_covers: usize,
        target_dir: &Path,
    ) -> Result<Vec<(String, PathBuf)>, DownloadCoversError> {
        // update valid collection ids collection if it is empty
        if self.valid_cids.is_empty() {
            self.update_collection_ids().await?;
        }

        // verify that the specified policy_id is a valid collection_id in the books API
        // endpoint
        if !self.valid_cids.contains(policy_id) {
            return Err(DownloadCoversError::InvalidId);
        }

        // seek through assets with a quantity > 0 until we have accumulated `num_covers` or
        // until we reach the end of the stream.
        let mut assets_stream = self.blockfrost_client.assets_policy_by_id_all(policy_id);
        let mut accumulated_assets = Vec::new();
        while accumulated_assets.len() < num_covers {
            let Some(result) = assets_stream.next().await else {
                break;
            };
            for asset in result? {
                if asset.quantity != "0" {
                    accumulated_assets.push(asset.asset);
                    if accumulated_assets.len() >= num_covers {
                        break;
                    }
                }
            }
        }

        // transform each asset_id into a pair of (src, media_type) representing the high-res
        // image for that asset
        let mut download_targets = Vec::new();
        for asset_id in accumulated_assets {
            let asset = self.blockfrost_client.assets_by_id(&asset_id).await?;
            let Some(metadata) = asset.onchain_metadata else {
                return Err(DownloadCoversError::MetadataMissing { asset_id });
            };
            let Some(files) = metadata.get("files") else {
                return Err(DownloadCoversError::MetadataFilesMissing { asset_id });
            };
            let Some(files) = files.as_array() else {
                return Err(DownloadCoversError::MetadataFileInvalid {
                    asset_id,
                    message: "the value for the `files` key must be an Array",
                });
            };
            if files.is_empty() {
                return Err(DownloadCoversError::MetadataFilesEmpty { asset_id });
            }
            let mut high_res_ipfs_asset = None;
            for file in files {
                let Some(file) = file.as_object() else {
                    return Err(DownloadCoversError::MetadataFileInvalid {
                        asset_id,
                        message: "each element of the `files` array must be an Object",
                    });
                };
                let Some(JsonValue::String(name)) = file.get("name") else {
                    return Err(DownloadCoversError::MetadataFileInvalid {
                        asset_id,
                        message: "each file in the files array must have a String `name` key",
                    });
                };
                let Some(JsonValue::String(media_type)) = file.get("mediaType") else {
                    return Err(DownloadCoversError::MetadataFileInvalid {
                        asset_id,
                        message: "each file in the files array must have a String `mediaType` key",
                    });
                };
                let Some(JsonValue::String(src)) = file.get("src") else {
                    return Err(DownloadCoversError::MetadataFileInvalid {
                        asset_id,
                        message: "each file in the files array must have a String `src` key",
                    });
                };
                if name == "High-Res Cover Image" {
                    high_res_ipfs_asset = Some((src, media_type));
                    break;
                }
            }
            let Some((src, media_type)) = high_res_ipfs_asset else {
                return Err(DownloadCoversError::MetadataFilesMissingHighResImage { asset_id });
            };
            // we now have an ipfs src and media_type for the high-res image for this asset

            // queue download
            let ext = extension_for(media_type);
            download_targets.push((src.clone(), target_dir.join(format!("{asset_id}.{ext}"))));
        }

        // start all downloads in parallel
        let downloaded_files = self.download_files(download_targets).await?;

        Ok(downloaded_files)
    }

    /// This function is an internal implementation detail of
    /// [`Client::download_covers_for_policy`] and will start the download process directly,
    /// given a Vec containing tuples of `(ipfs_path, dest_path)` without having to interact
    /// with BlockFrost.
    pub async fn download_files(
        &self,
        files: Vec<(String, PathBuf)>,
    ) -> Result<Vec<(String, PathBuf)>, DownloadCoversError> {
        let multi_progress = Arc::new(MultiProgress::new());
        let local_set = tokio::task::LocalSet::new();
        let files_ret = files.clone();

        // create a channel to send results of the download tasks to parent thread
        let (sender, mut receiver) = mpsc::channel(files.len());

        for (src, dest_path) in files {
            let cid = src_to_cid(src);
            // stat object so we can calibrate the progress bar
            let stat = match self.ipfs_client.files_stat(&cid).await {
                Ok(stat) => stat,
                Err(err) => Err(DownloadError::from(&cid, err))?,
            };

            // calculate true size of file minus overhead, possibly simulating random
            // incompletion of download if `simulate_early_kill` is true
            let mut total_size = stat.size;
            if self.simulate_early_kill {
                let mut rng = rand::thread_rng();
                total_size = (total_size as f64
                    * (&[1.0, 0.0, 0.9, 0.5, 0.3, 0.8])
                        .choose(&mut rng)
                        .copied()
                        .unwrap()) as u64;
            }

            let pb = if self.quiet {
                None
            } else {
                let pb = multi_progress.add(ProgressBar::new(total_size).with_position(0));
                let pb_style = ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-");
                pb.set_style(pb_style);
                Some(pb)
            };

            let ipfs_client = self.ipfs_client.clone();
            let sender = sender.clone();
            let slow = self.slow;

            local_set.spawn_local(async move {
                let result = download_single_file_with_progress(
                    &ipfs_client,
                    &cid,
                    &dest_path,
                    &pb,
                    total_size,
                    slow,
                )
                .await;
                // Send the result back to the main task
                if sender.send((cid, result)).await.is_err() {
                    eprintln!("Failed to send result back to the main task");
                }
            });
        }

        // drop the original transmitter so the channel can close once all tasks are done
        drop(sender);

        // run the local set until completion.
        local_set.await;

        // collect any errors from the download tasks and return, if applicable
        let mut errors = Vec::new();
        while let Some(result) = receiver.recv().await {
            if let (cid, Err(error)) = result {
                errors.push(DownloadError::from(cid, error));
            }
        }
        if !errors.is_empty() {
            return Err(DownloadCoversError::DownloadErrors(errors));
        }

        Ok(files_ret)
    }
}

/// Internal implementation detail of [`Client::download_files`] that handles a single file.
async fn download_single_file_with_progress(
    ipfs_client: &IpfsClient,
    cid: &str,
    dest_path: &Path,
    pb: &Option<ProgressBar>,
    total_size: u64,
    slow: bool,
) -> Result<(), DownloadErrorInner> {
    // check if file exists and its size
    let mut start_byte = 0;
    let file_exists = dest_path.exists();
    if file_exists {
        let metadata = tokio::fs::metadata(dest_path).await?;
        start_byte = metadata.len();

        // if file exists and is complete, set progress to 100% and return
        if start_byte == total_size {
            if let Some(pb) = pb {
                pb.finish_with_message("File already downloaded.");
            }
            return Ok(());
        }
    }

    if let Some(pb) = pb {
        pb.set_position(start_byte);
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(dest_path)
        .await?;

    // if the file exists but is incomplete, seek to the end of it
    if file_exists {
        file.seek(std::io::SeekFrom::End(0)).await?;
    }

    // determine the remaining length to download.
    let remaining_length = if total_size > start_byte {
        total_size - start_byte
    } else {
        if let Some(pb) = pb {
            pb.finish_with_message("File already downloaded.");
        }
        return Ok(());
    };

    // create the stream outside of the if condition.
    let stream = ipfs_client
        .cat_range(cid, start_byte as usize, remaining_length as usize)
        .boxed_local(); // Use boxed_local to box the stream that is not Send.

    // use the StreamExt trait to call next on the stream.
    futures::pin_mut!(stream); // pin the stream to be able to call `next`.

    while let Some(chunk) = stream.next().await {
        let data = chunk?;
        file.write_all(&data).await?;
        if let Some(pb) = pb {
            pb.inc(data.len() as u64);
        }
        if slow {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    file.flush().await?;

    let final_metadata = tokio::fs::metadata(dest_path).await?;

    if final_metadata.len() != total_size {
        return Err(DownloadErrorInner::CorruptDownload);
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Download complete.");
    }
    Ok(())
}

/// Implementation detail of [`Client::download_covers_for_policy`] that converts an
/// `ipfs://{cid}` path to an `/ipfs/{cid}` path.
///
/// Paths already conforming to this format should be unaffected. Everything else will
/// automatically be prefixed with `/ipfs/`.
pub fn src_to_cid<S: AsRef<str>>(src: S) -> String {
    let src = src.as_ref();
    if src.starts_with("/ipfs/") {
        return src.to_string();
    }
    let cid = if src.starts_with("ipfs://") {
        &src[7..]
    } else {
        &src[..]
    };
    format!("/ipfs/{cid}")
}

/// Implementation detail of [`Client::download_covers_for_policy`] that provides the proper
/// extension for a given image mimetype.
///
/// All common image formats are covered with default fallback to `png` for unknown formats.
///
/// The following image formats are supported:
/// - jpg
/// - png
/// - gif
/// - webp
/// - cr2
/// - tif
/// - bmp
/// - heif
/// - avif
/// - jxr
/// - psd
/// - ico
/// - ora
pub fn extension_for<S: AsRef<str>>(mime_string: S) -> &'static str {
    match mime_string.as_ref().to_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" | "image/vnd.compuserve.gif" => "gif",
        "image/webp" => "webp",
        "image/x-canon-cr2" | "image/cr2" => "cr2",
        "image/tiff" | "image/tif" => "tif",
        "image/bmp" | "image/vnd.microsoft.bitmap" => "bmp",
        "image/heif" => "heif",
        "image/avif" => "avif",
        "image/vnd.ms-photo" | "image/jxr" => "jxr",
        "image/vnd.adobe.photoshop" | "image/psd" => "psd",
        "image/vnd.microsoft.icon" | "image/ico" => "ico",
        "image/openraster" | "image/ora" => "ora",
        _ => "png",
    }
}

/// Used for testing, provides an appropriate runtime error if the `BLOCKFROST_PROJECT_ID` env
/// var is missing, and otherwise returns the project id.
#[cfg(test)]
pub fn load_project_id() -> String {
    std::env::var("BLOCKFROST_PROJECT_ID")
        .expect("environment variable `BLOCKFROST_PROJECT_ID` must be specified to run test suite")
}

/// Used for testing. Provides a temporary directory that will exist for the life of the
/// closure to which it is passed.
#[cfg(test)]
async fn with_temp_dir<F, Fut>(func: F) -> tokio::io::Result<()>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: futures::Future<Output = tokio::io::Result<()>>,
{
    let dir = tempdir::TempDir::new("book_io_download_tests")?; // Create a new temporary directory.
    let dir_path = dir.path().to_path_buf();

    // Run the async closure, passing the ownership of the PathBuf.
    func(dir_path).await?;

    // The temporary directory is automatically deleted when `dir` goes out of scope here.
    Ok(())
}

#[test]
fn test_src_to_cid() {
    assert_eq!(src_to_cid(String::from("ipfs://my-path")), "/ipfs/my-path");
    assert_eq!(
        src_to_cid("ipfs://some-longer-path"),
        "/ipfs/some-longer-path"
    );
    assert_eq!(src_to_cid("hello/world"), "/ipfs/hello/world");
    assert_eq!(src_to_cid("/ipfs/test"), "/ipfs/test");
}

#[test]
fn test_extension_for() {
    assert_eq!(extension_for("image/png"), "png");
    assert_eq!(extension_for("image/PNG"), "png");
    assert_eq!(extension_for("image/jpeg"), "jpg");
    assert_eq!(extension_for("image/vnd.microsoft.bitmap"), "bmp");
    assert_eq!(extension_for("image/bmp"), "bmp");
    assert_eq!(extension_for("image/vnd.adobe.photoshop"), "psd");
}

#[test]
fn test_client_with_project_id() {
    let blockfrost_project_id = load_project_id();
    Client::with_project_id(&blockfrost_project_id);
}

#[test]
fn test_invalid_project_id_does_not_panic() {
    Client::with_project_id("abcd");
}

#[test]
fn test_auto_load_project_id() {
    load_project_id();
    Client::new().unwrap();
}

#[tokio::test]
async fn test_download_covers() {
    load_project_id();
    const THE_WIZARD_TIM_POLICY_ID: &'static str =
        "c40ca49ac9fe48b86d6fd998645b5c8ac89a4e21e2cfdb9fdca3e7ac";

    with_temp_dir(|path| async move {
        let mut client = Client::new().unwrap();
        client.quiet = true;
        let res = client
            .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5, &path)
            .await
            .unwrap();
        assert_eq!(res.len(), 5);
        for (cid, path) in res {
            assert_eq!(cid.len(), 53);
            let metadata = std::fs::metadata(path).unwrap();
            assert_eq!(metadata.is_file(), true);
            assert_ne!(metadata.len(), 0);
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_download_covers_idempotent() {
    // checks idempotence by resuming a bunch of downloads that stopped at random points or
    // never started.
    load_project_id();
    const THE_WIZARD_TIM_POLICY_ID: &'static str =
        "c40ca49ac9fe48b86d6fd998645b5c8ac89a4e21e2cfdb9fdca3e7ac";

    with_temp_dir(|path| async move {
        let mut client = Client::new().unwrap();
        client.quiet = true;
        client.simulate_early_kill = true;
        // do partial downloads of a batch of 5
        let res = client
            .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5, &path)
            .await
            .unwrap();
        assert_eq!(res.len(), 5);
        for (cid, path) in res {
            assert_eq!(cid.len(), 53);
            let metadata = std::fs::metadata(path).unwrap();
            assert_eq!(metadata.is_file(), true);
        }
        client.simulate_early_kill = false;
        // finish everything in the partially finished batch of 5
        let res = client
            .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5, &path)
            .await
            .unwrap();
        assert_eq!(res.len(), 5);
        for (cid, path) in res {
            assert_eq!(cid.len(), 53);
            let metadata = std::fs::metadata(path).unwrap();
            assert_eq!(metadata.is_file(), true);
            assert_ne!(metadata.len(), 0);
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_download_covers_invalid_policy_id() {
    load_project_id();
    let mut client = Client::new().unwrap();
    client.quiet = true;
    assert!(matches!(
        client
            .download_covers_for_policy("bad-id", 3, &PathBuf::from("/tmp"))
            .await,
        Err(DownloadCoversError::InvalidId)
    ));
}
