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

pub const BOOK_API_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Deserialize, Debug)]
struct CollectionResponse {
    #[serde(rename = "type")]
    response_type: String,
    data: Vec<Collection>,
}

#[derive(Deserialize, Debug)]
struct Collection {
    collection_id: String,
    #[allow(unused)]
    description: String,
    blockchain: String,
    network: String,
}

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    blockfrost_client: BlockFrostApi,
    book_api_url: &'static str,
    valid_cids: HashSet<String>,
    slow: bool,
    simulate_early_kill: bool,
}

#[derive(Debug)]
pub enum DownloadErrorInner {
    IpfsError(ipfs_api::Error),
    IoError(std::io::Error),
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

#[derive(Debug)]
pub struct DownloadError {
    pub cid: String,
    pub error: DownloadErrorInner,
}

impl DownloadError {
    pub fn from<C: AsRef<str>, E: Into<DownloadErrorInner>>(cid: C, error: E) -> Self {
        DownloadError {
            cid: cid.as_ref().to_owned(),
            error: error.into(),
        }
    }
}

#[derive(Debug)]
pub enum UpdateCollectionIdsError {
    Request(reqwest::Error),
}

impl From<reqwest::Error> for UpdateCollectionIdsError {
    fn from(value: reqwest::Error) -> Self {
        UpdateCollectionIdsError::Request(value)
    }
}

#[derive(Debug)]
pub enum DownloadCoversError {
    UpdateCollectionIds(UpdateCollectionIdsError),
    InvalidId,
    BlockFrost(blockfrost::Error),
    MetadataMissing {
        asset_id: String,
    },
    MetadataFilesMissing {
        asset_id: String,
    },
    MetadataFileInvalid {
        asset_id: String,
        message: &'static str,
    },
    MetadataFilesEmpty {
        asset_id: String,
    },
    MetadataFilesMissingHighResImage {
        asset_id: String,
    },
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

#[derive(Debug, PartialEq, Eq)]
pub enum CreateClientError {
    MissingProjectId,
}

impl Client {
    pub fn new() -> Result<Self, CreateClientError> {
        let project_id =
            std::env::var("BLOCKFROST_PROJECT_ID").or(Err(CreateClientError::MissingProjectId))?;
        Ok(Client::with_project_id(&project_id))
    }

    pub fn with_project_id(blockfrost_project_id: &str) -> Self {
        // note that BlockFrost claims that their `new` method will panic if there is an
        // invalid project ID. I have tested this and it is actually incorrect, instead you see
        // this later as an error when you go to do something with the `BlockFrostApi` object,
        // so that is why this method is infallible.
        Client {
            ipfs_client: IpfsClient::default(),
            blockfrost_client: BlockFrostApi::new(blockfrost_project_id, Default::default()),
            book_api_url: BOOK_API_URL,
            valid_cids: HashSet::new(),
            slow: false,
            simulate_early_kill: false,
        }
    }

    pub async fn update_collection_ids(&mut self) -> Result<(), UpdateCollectionIdsError> {
        let response = reqwest::get(self.book_api_url).await?.error_for_status()?;
        let collection_response: CollectionResponse = response.json().await?;
        if collection_response.response_type != "success" {}
        self.valid_cids = collection_response
            .data
            .into_iter()
            .filter(|collection| {
                collection.blockchain == "cardano" && collection.network == "mainnet"
            })
            .map(|collection| collection.collection_id)
            .collect();
        Ok(())
    }

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

    async fn download_files(
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

            let pb = multi_progress.add(ProgressBar::new(total_size).with_position(0));
            let pb_style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-");
            pb.set_style(pb_style);

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

async fn download_single_file_with_progress(
    ipfs_client: &IpfsClient,
    cid: &str,
    dest_path: &Path,
    pb: &ProgressBar,
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
            pb.finish_with_message("File already downloaded.");
            return Ok(());
        }
    }

    pb.set_position(start_byte);

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
        pb.finish_with_message("File already downloaded.");
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
        pb.inc(data.len() as u64);
        file.write_all(&data).await?;
        if slow {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    file.flush().await?;

    let final_metadata = tokio::fs::metadata(dest_path).await?;

    if final_metadata.len() != total_size {
        return Err(DownloadErrorInner::CorruptDownload);
    }

    pb.finish_with_message("Download complete.");
    Ok(())
}

fn src_to_cid<S: AsRef<str>>(src: S) -> String {
    let src = src.as_ref();
    let cid = if src.starts_with("ipfs://") {
        &src[7..]
    } else {
        &src[..]
    };
    format!("/ipfs/{cid}")
}

fn extension_for<S: AsRef<str>>(mime_string: S) -> &'static str {
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

#[cfg(test)]
pub fn load_project_id() -> String {
    std::env::var("BLOCKFROST_PROJECT_ID")
        .expect("environment variable `BLOCKFROST_PROJECT_ID` must be specified to run test suite")
}

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
    assert!(matches!(
        client
            .download_covers_for_policy("bad-id", 3, &PathBuf::from("/tmp"))
            .await,
        Err(DownloadCoversError::InvalidId)
    ));
}
