use alloy_primitives::Address;
use celestia_rpc::Client;
use celestia_types::nmt::Namespace;
use std::sync::Arc;

/// Online client to fetch data from a Celestia network
#[derive(Clone)]
pub struct OnlineCelestiaProvider {
    /// The node client
    pub client: Arc<Client>,
    /// The namespace to fetch data from
    pub namespace: Namespace,
    /// The Blobstream contract address
    pub blobstream_address: Address,
}

impl OnlineCelestiaProvider {
    pub fn new(client: Client, namespace: Namespace, blobstream_address: Address) -> Self {
        OnlineCelestiaProvider {
            client: Arc::new(client),
            namespace,
            blobstream_address,
        }
    }
}

impl core::fmt::Debug for OnlineCelestiaProvider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OnlineCelestiaProvider")
            .field("namespace", &self.namespace)
            .field("blobstream_address", &self.blobstream_address)
            // Skip debugging the client field since it doesn't implement Debug
            .finish_non_exhaustive()
    }
}
