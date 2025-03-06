use alloy_primitives::{hex, keccak256, Bytes, FixedBytes, B256, U256};
use alloy_trie::{
    proof::{verify_proof, ProofVerificationError},
    Nibbles,
};
use celestia_types::{hash::Hash, MerkleProof, ShareProof};
use serde::{Deserialize, Serialize};

pub const DATA_COMMITMENTS_SLOT: u32 = 254;

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
            proof_nonce,
            storage_root,
            storage_proof,
        }
    }

    /// Serialize the struct to bytes using serde with a binary format
    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let bytes = bincode::serialize(self)?;
        Ok(bytes)
    }

    /// Deserialize from bytes back into the struct
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let deserialized = bincode::deserialize(bytes)?;
        Ok(deserialized)
    }
}

pub fn encode_data_root_tuple(height: u64, data_root: &Hash) -> Vec<u8> {
    // Create the result vector with 64 bytes capacity
    let mut result = Vec::with_capacity(64);

    // Pad the height to 32 bytes (convert to big-endian and pad with zeros)
    let height_bytes = height.to_be_bytes();

    // Add leading zeros (24 bytes of padding)
    result.extend_from_slice(&[0u8; 24]);

    // Add the 8-byte height
    result.extend_from_slice(&height_bytes);

    // Add the 32-byte data root
    result.extend_from_slice(data_root.as_bytes());

    result
}

/// Verify a storage proof for the state_dataCommitments mapping
pub fn verify_data_commitment_storage(
    root: B256,
    storage_proof: Vec<Bytes>,
    commitment_nonce: U256,
    expected_commitment: B256,
) -> Result<(), ProofVerificationError> {
    // Calculate the storage slot for state_dataCommitments[nonce]
    let slot = calculate_mapping_slot(DATA_COMMITMENTS_SLOT, commitment_nonce);
    println!(
        "Calculated slot for nonce {}: 0x{}",
        commitment_nonce,
        hex::encode(slot)
    );

    let nibbles = Nibbles::unpack(keccak256(slot));

    // Handle the RLP encoding by modifying the expected result
    // Add the 0xa0 prefix to match how it's stored on-chain
    let mut expected_with_prefix = Vec::with_capacity(33);
    expected_with_prefix.push(0xa0); // Add the RLP prefix
    expected_with_prefix.extend_from_slice(expected_commitment.as_slice());

    match verify_proof(root, nibbles, Some(expected_with_prefix), &storage_proof) {
        Ok(_) => {
            println!(
                "Succesfully verified storage proof for commitment {:?}",
                expected_commitment
            );
            Ok(())
        }
        Err(err) => return Err(err),
    }
}

/// Calculate the storage slot for a mapping with a uint256 key
pub fn calculate_mapping_slot(mapping_slot: u32, key: U256) -> B256 {
    let key_bytes = key.to_be_bytes::<32>();

    let slot_bytes = U256::from(mapping_slot).to_be_bytes::<32>();

    let mut concatenated = [0u8; 64];
    concatenated[0..32].copy_from_slice(&key_bytes);
    concatenated[32..64].copy_from_slice(&slot_bytes);

    alloy_primitives::keccak256(concatenated)
}
