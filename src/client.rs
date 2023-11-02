use blockfrost::{load, stream::StreamExt, BlockFrostApi};
use ipfs_api::IpfsClient;

pub const METADATA_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    blockfrost_client: BlockFrostApi,
    metadata_url: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

#[derive(Debug)]
pub enum GetMetadataError {
    BlockFrost(blockfrost::Error),
}

impl From<blockfrost::Error> for GetMetadataError {
    fn from(value: blockfrost::Error) -> Self {
        GetMetadataError::BlockFrost(value)
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
        Client {
            ipfs_client: IpfsClient::default(),
            blockfrost_client: BlockFrostApi::new(blockfrost_project_id, Default::default()),
            metadata_url: METADATA_URL,
        }
    }

    pub async fn download_covers_for_policy(
        &self,
        policy_id: &str,
        num_covers: usize,
    ) -> Result<CollectionMetadata, GetMetadataError> {
        let mut assets_stream = self.blockfrost_client.assets_policy_by_id_all(policy_id);
        //                                             ^ this should be named
        //                                             assets_by_policy_id_all, smh

        // seek through assets with a quantity > 0 until we have accumulated `num_covers` or
        // until we reach the end of the stream. We could use filter_map but this would result
        // in more allocations, and we only care about the first `num_covers` matching assets.
        let mut accumulated_assets = Vec::new();
        while accumulated_assets.len() < num_covers {
            let Some(result) = assets_stream.next().await else {
                break;
            };
            for asset in result? {
                // crazy that blockfrost exposes this merely as a String, such bad API
                // client design, should be some sort of integer, luckily for our
                // purposes we just need to check for equality with "0" so we are only
                // looking at unburned assets
                if asset.quantity != "0" {
                    accumulated_assets.push(asset);
                    if accumulated_assets.len() >= num_covers {
                        break;
                    }
                }
            }
        }

        // let assets = self
        //     .blockfrost_client
        //     .assets_by_id_all(policy_id)
        //     .take(10)
        //     .collect::<Vec<_>>()
        //     .await;
        // let txs = self
        //     .blockfrost_client
        //     .assets_transactions_all(&asset.asset)
        //     .take(10)
        //     .collect::<Vec<_>>()
        //     .await;
        // let metadata = self
        //     .blockfrost_client
        //     .transactions_metadata(
        //         "7d97631704481a7d38177423484fcf78964a29802db6b0d2880b814146364ee6",
        //     )
        //     .await?;
        println!("{:#?}", accumulated_assets);
        Ok(CollectionMetadata)
    }
}

#[cfg(test)]
pub fn load_project_id() -> String {
    std::env::var("BLOCKFROST_PROJECT_ID")
        .expect("environment variable `BLOCKFROST_PROJECT_ID` must be specified to run test suite")
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
    let client = Client::new().unwrap();
    client
        .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5)
        .await
        .unwrap();
}
