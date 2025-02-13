use alloc::boxed::Box;
use alloc::sync::Arc;
use alloy_primitives::{keccak256, Bytes};
use async_trait::async_trait;
use celestia_types::Commitment;
use hana_celestia::CelestiaProvider;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_proof::errors::OracleProviderError;
use kona_proof::Hint;

use crate::hint::HintWrapper;

/// An oracle-backed da storage.
#[derive(Debug, Clone)]
pub struct OracleCelestiaProvider<T: CommsClient> {
    oracle: Arc<T>,
}

impl<T: CommsClient + Clone> OracleCelestiaProvider<T> {
    /// Constructs a new `OracleBlobProvider`.
    pub fn new(oracle: Arc<T>) -> Self {
        Self { oracle }
    }
}

#[async_trait]
impl<T: CommsClient + Sync + Send> CelestiaProvider for OracleCelestiaProvider<T> {
    type Error = OracleProviderError;

    async fn blob_get(&self, height: u64, commitment: Commitment) -> Result<Bytes, Self::Error> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&height.to_le_bytes());
        encoded.extend_from_slice(commitment.hash());

        // See if we should perform blobstream verification logic here or in celestia.rs
        let hint = Hint::new(HintWrapper::CelestiaDA, encoded.clone());

        // // Fix the error mapping here
        hint.send(&*self.oracle).await?;

        let data = self
            .oracle
            .get(PreimageKey::new(
                *keccak256(encoded),
                PreimageKeyType::GlobalGeneric,
            ))
            .await?;

        Ok(data.into())
    }
}
