use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_provider::{Provider, RootProvider};
use alloy_rpc_types::{BlockNumberOrTag, Filter, FilterBlockOption, FilterSet};
use alloy_sol_types::{sol, SolEvent};
use celestia_rpc::Client;
use celestia_types::nmt::Namespace;
use std::sync::Arc;
use tracing::info;

// Geth has a default of 5000 block limit for filters
const FILTER_BLOCK_RANGE: u64 = 5000;

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

    /// Find the data commitment that contains the given Celestia height
    pub async fn find_data_commitment(
        &self,
        celestia_height: u64,
        eth_provider: &RootProvider,
    ) -> Result<SP1BlobstreamDataCommitmentStored, Box<dyn std::error::Error>> {
        let eth_block_height = eth_provider.get_block_number().await?;
        // Calculate event signature manually for reliability
        let event_signature = "DataCommitmentStored(uint256,uint64,uint64,bytes32)";
        let event_selector = keccak256(event_signature.as_bytes());
        let topic0: FilterSet<B256> = vec![event_selector.into()].into();

        // Start from the given Ethereum block height and scan backwards
        let mut end = eth_block_height;
        let mut start = if end > FILTER_BLOCK_RANGE {
            end - FILTER_BLOCK_RANGE
        } else {
            0
        };

        loop {
            info!("Scanning blocks from {} to {} for events", start, end);

            // Create filter for DataCommitmentStored events
            let filter = Filter {
                block_option: FilterBlockOption::Range {
                    from_block: Some(BlockNumberOrTag::Number(start.into())),
                    to_block: Some(BlockNumberOrTag::Number(end.into())),
                },
                address: vec![self.blobstream_address].into(),
                topics: [
                    topic0.clone(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ],
            };

            // Get logs using the client reference
            let logs = eth_provider.get_logs(&filter).await?;
            info!("Found {} logs in block range", logs.len());

            // Parse logs using the generated event type
            for log in logs {
                // Try to decode the log using SP1Blobstream's generated event decoder
                if let Ok(event) =
                    SP1Blobstream::DataCommitmentStored::decode_log(&log.clone().into(), true)
                {
                    // Check if this event contains the celestia_height
                    if event.startBlock <= celestia_height && celestia_height < event.endBlock {
                        let stored_event = SP1BlobstreamDataCommitmentStored {
                            proof_nonce: event.proofNonce,
                            start_block: event.startBlock,
                            end_block: event.endBlock,
                            data_commitment: event.dataCommitment,
                        };

                        info!(
                            "Found Data Root submission event block_number={} proof_nonce={} start={} end={}",
                            log.clone().block_number.unwrap(),
                            stored_event.proof_nonce,
                            stored_event.start_block,
                            stored_event.end_block
                        );

                        return Ok(stored_event);
                    }
                }
            }

            // If we've reached the beginning of the chain, stop
            if start == 0 {
                return Err("No matching event found for the given Celestia height".into());
            }

            // Move to the previous batch
            end = start;
            start = if end > FILTER_BLOCK_RANGE {
                end - FILTER_BLOCK_RANGE
            } else {
                0
            };
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
