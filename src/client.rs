use ipfs_api::IpfsClient;

#[derive(Clone)]
pub struct Client {
    ipfs_client: IpfsClient,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            ipfs_client: IpfsClient::default(),
        }
    }
}

impl Client {
    pub fn new() -> Self {
        Client::default()
    }
}
