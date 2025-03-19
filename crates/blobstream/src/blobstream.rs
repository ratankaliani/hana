use std::{boxed::Box, println};

use alloc::vec::Vec;
use alloy_primitives::{keccak256, Bytes as AlloyBytes, FixedBytes, B256, U256};
use alloy_sol_types::sol;
use alloy_trie::{
    proof::{verify_proof, ProofVerificationError},
    Nibbles,
};
use anyhow::Error;
use bytes::Bytes;
use celestia_proto::{
    celestia::blob::v1::MsgPayForBlobs, cosmos::tx::v1beta1::Tx, proto::blob::v1::IndexWrapper,
};
use celestia_types::{
    hash::Hash, row_namespace_data::NamespaceData, Commitment, MerkleProof, ShareProof,
};
use serde::{Deserialize, Serialize};
use sov_celestia_adapter::{
    parse_pfb_namespace,
    shares::{NamespaceGroup, Share as SovShare, INFO_BYTE_LEN},
    verifier::PFB_NAMESPACE,
    TxPosition,
};

/////// Contract ///////

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    contract SP1Blobstream {
        bool public frozen;
        uint64 public latestBlock;
        uint256 public state_proofNonce;
        mapping(uint64 => bytes32) public blockHeightToHeaderHash;
        mapping(uint256 => bytes32) public state_dataCommitments;
        uint64 public constant DATA_COMMITMENT_MAX = 10000;
        bytes32 public blobstreamProgramVkey;
        address public verifier;

        event DataCommitmentStored(
            uint256 proofNonce,
            uint64 indexed startBlock,
            uint64 indexed endBlock,
            bytes32 indexed dataCommitment
        );

        function commitHeaderRange(bytes calldata proof, bytes calldata publicValues) external;
    }
}

/// Represents the stored data commitment event from Blobstream
#[derive(Debug, Clone)]
pub struct SP1BlobstreamDataCommitmentStored {
    pub proof_nonce: U256,
    pub start_block: u64,
    pub end_block: u64,
    pub data_commitment: B256,
}

impl std::fmt::Display for SP1BlobstreamDataCommitmentStored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SP1BlobstreamDataCommitmentStored {{ proof_nonce: {}, start_block: {}, end_block: {}, data_commitment: {} }}",
            self.proof_nonce, self.start_block, self.end_block, self.data_commitment)
    }
}

pub const DATA_COMMITMENTS_SLOT: u32 = 254;

/// A structure containing a Celestia Blob and its corresponding proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobstreamProof {
    /// The data root to verify the proof against
    pub data_root: Hash,
    /// The data commitment from Blobstream to verify against
    pub data_commitment: FixedBytes<32>,
    /// The Data Root Tuple Inclusion proof
    pub data_root_tuple_proof: MerkleProof,
    /// The proof for the blob's inclusion
    pub share_proof: ShareProof,
    /// PFB Namespace shares containing the shares for our PFB
    pub pfb_data: NamespaceData,
    /// The proof_nonce in blobstream
    pub proof_nonce: U256,
    /// The storage root to verify against
    pub storage_root: B256,
    /// The storage proof for the state_dataCommitments mapping slot in Blobstream
    pub storage_proof: Vec<AlloyBytes>,
}

impl BlobstreamProof {
    /// Create a new OraclePayload instance
    pub fn new(
        data_root: Hash,
        data_commitment: FixedBytes<32>,
        data_root_tuple_proof: MerkleProof,
        share_proof: ShareProof,
        pfb_data: NamespaceData,
        proof_nonce: U256,
        storage_root: B256,
        storage_proof: Vec<AlloyBytes>,
    ) -> Self {
        Self {
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
    storage_proof: Vec<AlloyBytes>,
    commitment_nonce: U256,
    expected_commitment: B256,
) -> Result<(), ProofVerificationError> {
    // Calculate the storage slot for state_dataCommitments[nonce]
    let slot = calculate_mapping_slot(DATA_COMMITMENTS_SLOT, commitment_nonce);

    let nibbles = Nibbles::unpack(keccak256(slot));

    // Handle the RLP encoding by modifying the expected result
    // Add the 0xa0 prefix to match how it's stored on-chain
    let mut expected_with_prefix = Vec::with_capacity(33);
    expected_with_prefix.push(0xa0); // Add the RLP prefix
    expected_with_prefix.extend_from_slice(expected_commitment.as_slice());

    match verify_proof(root, nibbles, Some(expected_with_prefix), &storage_proof) {
        Ok(_) => Ok(()),
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

pub fn find_pfb_with_commitment(
    namespace_data: &NamespaceData,
    target_commitment: &Commitment,
) -> Result<Vec<(MsgPayForBlobs, (usize, usize))>, anyhow::Error> {
    let shares_vec: Vec<SovShare> = namespace_data
        .rows
        .iter()
        .flat_map(|row| {
            row.shares.iter().enumerate().map(|(_index, share)| {
                println!(
                    "Start index of row in namespace: {:?}",
                    row.proof.start_idx()
                );

                println!("find_pfb_share: {:?}", share.data());

                let bytes = bytes::Bytes::copy_from_slice(share.data()); // Adjust based on how to extract data

                SovShare::new(bytes)
            })
        })
        .collect::<Vec<SovShare>>();

    let group = NamespaceGroup::from_shares(shares_vec);

    let pfbs = match parse_pfb_namespace(group) {
        Ok(pfbs) => pfbs,
        Err(err) => return Err(err),
    };

    println!("PFBs size: {:?}", pfbs.len());

    let mut matched_pfbs = Vec::new();
    for pfb in &pfbs {
        println!("PFB share commitments: {:?}", pfb.0.share_commitments);
        for commitment in &pfb.0.share_commitments {
            if commitment == target_commitment.hash() {
                let msg = MsgPayForBlobs {
                    signer: pfb.0.signer.clone(),
                    namespaces: pfb.0.namespaces.clone(),
                    blob_sizes: pfb.0.blob_sizes.clone(),
                    share_commitments: pfb.0.share_commitments.clone(),
                    share_versions: pfb.0.share_versions.clone(),
                };

                println!("Offset: {:?}", pfb.1.start_offset);
                let range: (usize, usize) = (pfb.1.share_range.start, pfb.1.share_range.end);

                matched_pfbs.push((msg, range));
            }
        }
    }

    if matched_pfbs.len() == 0 {
        return Err(Error::msg("no matching pfbs found"));
    }

    Ok(matched_pfbs)
}
