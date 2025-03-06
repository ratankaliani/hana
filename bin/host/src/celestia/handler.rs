//! [HintHandler] for the [CelestiaaChainHost].

use alloy_primitives::{hex, keccak256, Bytes, B256};
use alloy_provider::Provider;
use alloy_trie::{proof::verify_proof, Nibbles};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use celestia_rpc::{blobstream::BlobstreamClient, BlobClient, HeaderClient, ShareClient};
use celestia_types::Commitment;
use hana_oracle::{
    hint::HintWrapper,
    payload::{
        calculate_mapping_slot, encode_data_root_tuple, verify_data_commitment_storage,
        OraclePayload, DATA_COMMITMENTS_SLOT,
    },
};
use kona_host::{
    single::SingleChainHintHandler, HintHandler, OnlineHostBackendCfg, SharedKeyValueStore,
};
use kona_preimage::{PreimageKey, PreimageKeyType};
use kona_proof::Hint;
use tracing::info;

use crate::celestia::cfg::CelestiaChainHost;

/// The [HintHandler] for the [CelestiaChainHost].
#[derive(Debug, Clone, Copy)]
pub struct CelestiaChainHintHandler;

#[async_trait]
impl HintHandler for CelestiaChainHintHandler {
    type Cfg = CelestiaChainHost;

    async fn fetch_hint(
        hint: Hint<<Self::Cfg as OnlineHostBackendCfg>::HintType>,
        cfg: &Self::Cfg,
        providers: &<Self::Cfg as OnlineHostBackendCfg>::Providers,
        kv: SharedKeyValueStore,
    ) -> Result<()> {
        match hint.ty {
            HintWrapper::Standard(standard_hint) => {
                let inner_hint = Hint {
                    ty: standard_hint,
                    data: hint.data,
                };

                match SingleChainHintHandler::fetch_hint(
                    inner_hint,
                    &cfg.single_host.clone(),
                    &providers.inner_providers,
                    kv,
                )
                .await
                {
                    Ok(_) => (),
                    Err(err) => anyhow::bail!("Standard Hint processing error {}", err),
                }
            }
            HintWrapper::CelestiaDA => {
                ensure!(hint.data.len() == 40, "Invalid hint data length");

                let height = u64::from_le_bytes(hint.data[0..8].try_into().unwrap());

                let hash_array: [u8; 32] =
                    hint.data[8..40].try_into().expect("Slice must be 32 bytes");
                let commitment = Commitment::new(hash_array);

                let blob = match providers
                    .celestia
                    .client
                    .blob_get(height, providers.celestia.namespace, commitment)
                    .await
                {
                    Ok(blob) => blob,
                    Err(e) => anyhow::bail!("celestia blob not found: {:#}", e),
                };

                // Fetch the block's data root
                println!("Fetching header for height {:?}", height);
                let header = providers
                    .celestia
                    .client
                    .header_get_by_height(height)
                    .await?;

                let data_root = header.dah.hash();

                let eds_row_roots = header.dah.row_roots();
                let eds_size: u64 = eds_row_roots.len().try_into().unwrap();
                let ods_size: u64 = eds_size / 2;

                let index = blob.index.unwrap();
                let first_row_index: u64 = index.div_ceil(eds_size) - 1;
                let start_index = blob.index.unwrap() - (first_row_index * ods_size);
                let end_index = start_index + blob.shares_len() as u64;
                println!("Getting range proof {:?}-{:?}", start_index, end_index);

                let share_proof = providers
                    .celestia
                    .client
                    .share_get_range(&header, start_index, end_index)
                    .await
                    .expect("Failed getting share proof")
                    .proof;

                // validate the proof before placing it on the KV store
                share_proof
                    .verify(data_root)
                    .expect("failed to verify share proof against data root");

                let event = providers
                    .celestia
                    .find_data_commitment(height, providers.l1())
                    .await
                    .unwrap();

                info!("Found SP1 Data Commitment Stored Event: {:?}", event);

                let data_root_proof = providers
                    .celestia
                    .client
                    .get_data_root_tuple_inclusion_proof(height, event.start_block, event.end_block)
                    .await?;

                let encoded_data_root_tuple = encode_data_root_tuple(height, &data_root);

                data_root_proof
                    .verify(encoded_data_root_tuple, *event.data_commitment.clone())
                    .expect("failed to verify data root tuple inclusion proof");

                // TODO: Add More info here
                info!("Verified Share and Data Root Tuple Proofs");

                let slot = calculate_mapping_slot(DATA_COMMITMENTS_SLOT, event.proof_nonce);
                println!("Slot raw bytes: {:?}", slot.as_slice());
                println!("Slot hex: 0x{}", hex::encode(slot));

                // Calculate the hash of the slot for the proof verification
                let slot_hash = keccak256(slot);
                println!("Slot hash: 0x{}", hex::encode(slot_hash));

                println!(
                    "Storage slot for state_dataCommitments[{}]: 0x{}",
                    event.proof_nonce, slot
                );

                let slot_b256 = B256::from_slice(slot.as_slice());

                let proof_response = providers
                    .l1()
                    .get_proof(providers.celestia.blobstream_address, vec![slot_b256])
                    .await?;

                // Get the storage value directly
                let storage_value = providers
                    .l1()
                    .get_storage_at(providers.celestia.blobstream_address, slot.into())
                    .await?;

                println!("Direct storage value: 0x{}", storage_value.to_string());

                // After getting the proof_response
                println!(
                    "Storage proof value: {:?}",
                    proof_response.storage_proof[0].value
                );
                println!(
                    "Storage proof key: {:?}",
                    proof_response.storage_proof[0].key
                );

                let proof_bytes: Vec<Bytes> = proof_response
                    .storage_proof
                    .into_iter()
                    .flat_map(|proof| {
                        // Extract the proof field and apply any needed transformations
                        proof.proof.into_iter().map(|bytes| {
                            // You can apply transformations here if needed
                            // For example: Bytes::from(some_transformation(bytes))
                            // But in this case, we can just return the bytes directly
                            bytes
                        })
                    })
                    .collect();

                let nibbles = Nibbles::unpack(keccak256(slot));
                let result = verify_proof(
                    proof_response.storage_hash,
                    nibbles,
                    Some(event.data_commitment.to_vec()),
                    &proof_bytes,
                );
                println!("Direct verification result: {:?}", result);

                match verify_data_commitment_storage(
                    proof_response.storage_hash,
                    proof_bytes.clone(),
                    event.proof_nonce,
                    event.data_commitment,
                ) {
                    Ok(_) => println!(
                        "Succesfully verified storage proof for Blobstream data commitment"
                    ),
                    Err(err) => anyhow::bail!("Error verifying storage proof {}", err),
                }

                let mut kv_lock = kv.write().await;

                let celestia_commitment_hash = keccak256(&hint.data);

                let payload = OraclePayload::new(
                    Bytes::from(blob.data),
                    data_root,
                    event.data_commitment,
                    data_root_proof,
                    share_proof,
                    event.proof_nonce,
                    proof_response.storage_hash.clone(),
                    proof_bytes,
                )
                .to_bytes()
                .expect("failed to serialize celestia oracle payload");

                // store the blob data as a the preimage behind the hash of the height + blob commitment
                kv_lock.set(
                    PreimageKey::new(*celestia_commitment_hash, PreimageKeyType::GlobalGeneric)
                        .into(),
                    payload.into(),
                )?;
            }
        }
        Ok(())
    }
}
