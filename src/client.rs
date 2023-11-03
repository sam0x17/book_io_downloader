use blockfrost::{stream::StreamExt, BlockFrostApi, JsonValue};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use ipfs_api::IpfsClient;
use ipfs_api_backend_hyper::IpfsApi;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

pub const METADATA_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    blockfrost_client: BlockFrostApi,
    metadata_url: &'static str,
    slow: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

#[derive(Debug)]
pub enum DownloadErrorInner {
    IpfsError(ipfs_api::Error),
    IoError(std::io::Error),
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
pub enum DownloadCoversError {
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
        // so that is why this method is infallible. I had hoped to be able to return an
        // `InvalidProjectId` error variant here but alas, no.
        Client {
            ipfs_client: IpfsClient::default(),
            blockfrost_client: BlockFrostApi::new(blockfrost_project_id, Default::default()),
            metadata_url: METADATA_URL,
            slow: false,
        }
    }

    pub async fn download_covers_for_policy(
        &self,
        policy_id: &str,
        num_covers: usize,
        target_dir: &Path,
    ) -> Result<CollectionMetadata, DownloadCoversError> {
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
        self.download_files(download_targets).await?;

        Ok(CollectionMetadata)
    }

    async fn download_files(
        &self,
        files: Vec<(String, PathBuf)>,
    ) -> Result<(), DownloadCoversError> {
        let multi_progress = Arc::new(MultiProgress::new());
        let local_set = tokio::task::LocalSet::new(); // Create a LocalSet for local tasks.

        // create a channel to send results of the download tasks to parent thread
        let (tx, mut rx) = mpsc::channel(files.len());

        for (src, dest_path) in files {
            let cid = src_to_cid(src);
            // stat object so we can calibrate the progress bar
            let stat = match self.ipfs_client.object_stat(&cid).await {
                Ok(stat) => stat,
                Err(err) => Err(DownloadError::from(&cid, err))?,
            };
            let pb = multi_progress.add(ProgressBar::new(stat.cumulative_size).with_position(0));
            let pb_style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-");
            pb.set_style(pb_style);

            let ipfs_client = self.ipfs_client.clone();
            let tx = tx.clone();
            let slow = self.slow;

            local_set.spawn_local(async move {
                let result =
                    download_single_file_with_progress(&ipfs_client, &cid, &dest_path, &pb, slow)
                        .await;
                // Send the result back to the main task
                if tx.send((cid, result)).await.is_err() {
                    eprintln!("Failed to send result back to the main task");
                }
            });
        }

        // drop the original transmitter so the channel can close once all tasks are done
        drop(tx);

        // run the local set until completion.
        local_set.await;

        // collect any errors from the download tasks and return, if applicable
        let mut errors = Vec::new();
        while let Some(result) = rx.recv().await {
            if let (cid, Err(error)) = result {
                errors.push(DownloadError::from(cid, error));
            }
        }
        if !errors.is_empty() {
            return Err(DownloadCoversError::DownloadErrors(errors));
        }

        Ok(())
    }
}

async fn download_single_file_with_progress(
    ipfs_client: &IpfsClient,
    cid: &str,
    dest_path: &Path,
    pb: &ProgressBar,
    slow: bool,
) -> Result<(), DownloadErrorInner> {
    let mut file = File::create(dest_path).await?;
    let mut stream = ipfs_client.cat(&cid); // do this in outer fn and stat first to get total size

    while let Some(chunk) = stream.next().await {
        let data = chunk?;
        pb.inc(data.len() as u64);
        file.write_all(&data).await?;
        if slow {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    file.flush().await?;

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
    let mut client = Client::new().unwrap();
    client.slow = false;
    client
        .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5, &PathBuf::from("/tmp"))
        .await
        .unwrap();
}
