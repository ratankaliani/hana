use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloy_primitives::{keccak256, Bytes};
use async_trait::async_trait;
use celestia_types::Commitment;
use hana_blobstream::blobstream::{
    encode_data_root_tuple, find_pfb_with_commitment, verify_data_commitment_storage,
};
use hana_celestia::CelestiaProvider;
use hashbrown::HashSet;
use kona_preimage::errors::PreimageOracleError;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_proof::errors::OracleProviderError;
use kona_proof::Hint;
use tracing::info;

use crate::hint::HintWrapper;
use crate::payload::OraclePayload;

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

        // Perform Inclusion checks against the data root

        let hint = Hint::new(HintWrapper::CelestiaDA, encoded.clone());

        hint.send(&*self.oracle).await?;

        let oracle_result = self
            .oracle
            .get(PreimageKey::new(
                *keccak256(encoded),
                PreimageKeyType::GlobalGeneric,
            ))
            .await?;

        let payload = OraclePayload::from_bytes(&oracle_result)
            .expect("Failed to deserialize Celestia Oracle Payload");

        // throw the shares from our proof share into a hash_map then verify our namespace data
        // contains the shares from our proof to our PFB
        // NOTE: This will be simpliefied and removed to provide the exact shares in advance
        // once compact share code is upstreamed to celestia_types crate
        let proof_shares: HashSet<&[u8; 512]> = payload.share_proof.shares().iter().collect();

        let mut found_shares = 0;

        for row in &payload.pfb_data.rows {
            for share in &row.shares {
                if proof_shares.contains(share.data()) {
                    found_shares += 1;
                }
            }
        }

        // Calculate remaining shares (if needed)
        let remaining_shares = payload.share_proof.shares().len() - found_shares;
        info!("Remaining shares: {:?}", remaining_shares);
        if remaining_shares > 0 {
            return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
                String::from("proof shares not found to be in namespace data"),
            )));
        }

        let pfbs = find_pfb_with_commitment(&payload.pfb_data, &commitment)
            .expect("Failed to find pfb with commitment");

        for pfb in pfbs {
            if pfb.0.share_commitments[0] != commitment.hash() {
                return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
                    String::from("pfb did not contain the right commitment"),
                )));
            }
        }

        match payload.share_proof.verify(payload.data_root) {
            Ok(_) => info!("Celestia blobs ShareProof succesfully verified"),
            Err(err) => {
                return Err(OracleProviderError::Preimage(PreimageOracleError::Other(
                    err.to_string(),
                )))
            }
        }

        let encoded_data_root_tuple = encode_data_root_tuple(height, &payload.data_root);

        payload
            .data_root_tuple_proof
            .verify(encoded_data_root_tuple, *payload.data_commitment)
            .expect("Failed to verify data root tuple proof");

        verify_data_commitment_storage(
            payload.storage_root,
            payload.storage_proof,
            payload.proof_nonce,
            payload.data_commitment,
        )
        .expect("Failed to verify data commitment against Blobstream storage slot");

        Ok(payload.blob)
    }
}
