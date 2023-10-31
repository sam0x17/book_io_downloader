use std::string::FromUtf8Error;

use hyper::{body::to_bytes, client::HttpConnector, Body, Uri};
use ipfs_api::IpfsClient;

pub const METADATA_URL: &'static str = "https://api.book.io/api/v0/collections";

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
    http_client: hyper::Client<HttpConnector, Body>,
    metadata_url: &'static str,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            ipfs_client: IpfsClient::default(),
            http_client: hyper::Client::default(),
            metadata_url: METADATA_URL,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollectionMetadata;

#[derive(Debug)]
pub enum GetMetadataError {
    Network(hyper::Error),
    MalformedResponseUtf8(FromUtf8Error),
}

impl From<hyper::Error> for GetMetadataError {
    fn from(value: hyper::Error) -> Self {
        GetMetadataError::Network(value)
    }
}

impl From<FromUtf8Error> for GetMetadataError {
    fn from(value: FromUtf8Error) -> Self {
        GetMetadataError::MalformedResponseUtf8(value)
    }
}

impl Client {
    pub fn new() -> Self {
        Client::default()
    }

    pub async fn get_metadata(&self) -> Result<CollectionMetadata, GetMetadataError> {
        let future = self.http_client.get(Uri::from_static(self.metadata_url));
        let response = future.await?;
        let body_bytes = to_bytes(response.into_body()).await?;
        let body_string = String::from_utf8(body_bytes.to_vec())?;
        println!("{body_string}");
        Ok(CollectionMetadata)
    }
}
