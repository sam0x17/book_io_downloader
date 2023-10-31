use ipfs_api::IpfsClient;
use std::string::FromUtf8Error;

pub const METADATA_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    metadata_url: &'static str,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            ipfs_client: IpfsClient::default(),
            metadata_url: METADATA_URL,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

#[derive(Debug)]
pub enum GetMetadataError {
    ToDo,
}

impl Client {
    pub fn new() -> Self {
        Client::default()
    }

    pub async fn get_metadata(&self) -> Result<CollectionMetadata, GetMetadataError> {
        Ok(CollectionMetadata)
    }
}
