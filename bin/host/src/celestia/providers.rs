use crate::celestia::OnlineCelestiaProvider;
use alloy_provider::RootProvider;
use kona_host::single::SingleChainProviders;
use kona_providers_alloy::{OnlineBeaconClient, OnlineBlobProvider};
use op_alloy_network::Optimism;

/// The combined providers for Celestia and single chain operations
#[derive(Debug, Clone)]
pub struct CelestiaChainProviders {
    /// The original single chain providers
    pub inner_providers: SingleChainProviders,
    /// The Celestia provider
    pub celestia: OnlineCelestiaProvider,
}

impl CelestiaChainProviders {
    /// Create a new instance of CelestiaChainProviders
    pub fn new(inner_providers: SingleChainProviders, celestia: OnlineCelestiaProvider) -> Self {
        Self {
            inner_providers,
            celestia,
        }
    }

    /// Access the L1 provider from the inner providers
    pub fn l1(&self) -> &RootProvider {
        &self.inner_providers.l1
    }

    /// Access the blob provider from the inner providers
    pub fn blobs(&self) -> &OnlineBlobProvider<OnlineBeaconClient> {
        &self.inner_providers.blobs
    }

    /// Access the L2 provider from the inner providers
    pub fn l2(&self) -> &RootProvider<Optimism> {
        &self.inner_providers.l2
    }
}

impl From<CelestiaChainProviders> for SingleChainProviders {
    fn from(providers: CelestiaChainProviders) -> Self {
        providers.inner_providers
    }
}
