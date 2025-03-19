use alloc::{boxed::Box, vec::Vec};
use alloy_primitives::{Bytes, FixedBytes, B256, U256};
use celestia_types::{hash::Hash, row_namespace_data::NamespaceData, MerkleProof, ShareProof};
use serde::{Deserialize, Serialize};

/// A structure containing a Celestia Blob and its corresponding proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePayload {
    /// The Celestia blob data
    pub blob: Bytes,
    /// The data root to verify the proof against
    pub data_root: Hash,
    /// The data commitment from Blobstream to verify against
    pub data_commitment: FixedBytes<32>,
    /// The Data Root Tuple Inclusion proof
    pub data_root_tuple_proof: MerkleProof,
    /// The proof for the blob's inclusion
    pub share_proof: ShareProof,
    /// The pfb namespace data for the shares with our PFB
    pub pfb_data: NamespaceData,
    /// The proof_nonce in blobstream
    pub proof_nonce: U256,
    /// The storage root to verify against
    pub storage_root: B256,
    /// The storage proof for the state_dataCommitments mapping slot in Blobstream
    pub storage_proof: Vec<Bytes>,
}

impl OraclePayload {
    /// Create a new OraclePayload instance
    pub fn new(
        blob: Bytes,
        data_root: Hash,
        data_commitment: FixedBytes<32>,
        data_root_tuple_proof: MerkleProof,
        share_proof: ShareProof,
        pfb_data: NamespaceData,
        proof_nonce: U256,
        storage_root: B256,
        storage_proof: Vec<Bytes>,
    ) -> Self {
        Self {
            blob,
            data_root,
            data_commitment,
            data_root_tuple_proof,
            share_proof,
            pfb_data,
            proof_nonce,
            storage_root,
            storage_proof,
        }
    }

    /// Serialize the struct to bytes using serde with a binary format
    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn core::error::Error>> {
        let bytes = bincode::serialize(self)?;
        Ok(bytes)
    }

    /// Deserialize from bytes back into the struct
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn core::error::Error>> {
        let deserialized = bincode::deserialize(bytes)?;
        Ok(deserialized)
    }
}
