use blockfrost::{load, BlockFrostApi};
use ipfs_api::IpfsClient;

pub const METADATA_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    blockfrost_client: BlockFrostApi,
    metadata_url: &'static str,
}

fn build_block_frost_api(project_id: &str) -> blockfrost::Result<BlockFrostApi> {
    let configurations = load::configurations_from_env()?;
    let project_id = configurations[project_id].as_str().unwrap();
    let api = BlockFrostApi::new(project_id, Default::default());
    Ok(api)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

#[derive(Debug)]
pub enum GetMetadataError {
    ToDo,
}

#[derive(Debug)]
pub enum CreateClientError {
    MissingBlockFrostProjectId,
    BlockFrostError(blockfrost::Error),
}

impl From<blockfrost::Error> for CreateClientError {
    fn from(value: blockfrost::Error) -> Self {
        CreateClientError::BlockFrostError(value)
    }
}

impl Client {
    pub fn new(
        block_frost_project_id: Option<&'static str>,
        metadata_url: Option<&'static str>,
    ) -> Result<Self, CreateClientError> {
        Ok(Client {
            ipfs_client: IpfsClient::default(),
            blockfrost_client: build_block_frost_api(
                block_frost_project_id.ok_or(CreateClientError::MissingBlockFrostProjectId)?,
            )?
            .into(),
            metadata_url: metadata_url.unwrap_or(METADATA_URL),
        })
    }

    pub async fn get_metadata(&self) -> Result<CollectionMetadata, GetMetadataError> {
        Ok(CollectionMetadata)
    }
}
