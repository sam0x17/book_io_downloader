use hyper::{client::HttpConnector, Body};
use ipfs_api::IpfsClient;

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    hyper_client: hyper::Client<HttpConnector, Body>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            ipfs_client: IpfsClient::default(),
            hyper_client: hyper::Client::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

impl Client {
    pub fn new() -> Self {
        Client::default()
    }

    pub async fn get_collection_metadata(&self) -> Result<CollectionMetadata, ()> {
        Ok(CollectionMetadata)
    }
}
