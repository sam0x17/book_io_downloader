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
    ToDo,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CreateClientError {
    MissingProjectId,
    InvalidProjectId,
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

    pub async fn get_metadata(&self) -> Result<CollectionMetadata, GetMetadataError> {
        Ok(CollectionMetadata)
    }

    pub async fn is_collection_id_valid(
        &self,
        collection_id: &str,
    ) -> Result<bool, blockfrost::Error> {
        self.blockfrost_client
            .assets_all()
            .any(|assets| async move {
                let assets = asset.unwrap();
                asset.hello()
            })
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
