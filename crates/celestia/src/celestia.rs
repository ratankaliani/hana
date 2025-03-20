//! [CelestiaDADataSource] an implementation of the [DataAvailabilityProvider] trait.

use crate::source::CelestiaDASource;
use crate::traits::CelestiaProvider;

use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use celestia_types::Commitment;
use kona_derive::{
    errors::{PipelineError, PipelineErrorKind},
    sources::EthereumDataSource,
    traits::{BlobProvider, ChainProvider, DataAvailabilityProvider},
    types::PipelineResult,
};
use kona_protocol::BlockInfo;
/// A factory for creating a Celestia data source provider.
#[derive(Debug, Clone)]
pub struct CelestiaDADataSource<C, B, A>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Send + Clone,
    A: CelestiaProvider + Send + Clone,
{
    /// The blob source.
    pub ethereum_source: EthereumDataSource<C, B>,
    /// The celestia source.
    pub celestia_source: CelestiaDASource<A>,
}

impl<C, B, A> CelestiaDADataSource<C, B, A>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
    A: CelestiaProvider + Send + Clone + Debug,
{
    /// Creates a [CelestiaDADataSource] from the given sources.
    pub const fn new(
        ethereum_source: EthereumDataSource<C, B>,
        celestia_source: CelestiaDASource<A>,
    ) -> Self {
        Self {
            ethereum_source,
            celestia_source,
        }
    }
}

#[async_trait]
impl<C, B, A> DataAvailabilityProvider for CelestiaDADataSource<C, B, A>
where
    C: ChainProvider + Send + Sync + Clone + Debug,
    B: BlobProvider + Send + Sync + Clone + Debug,
    A: CelestiaProvider + Send + Sync + Clone + Debug,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> PipelineResult<Self::Item> {
        // Feth Blob pointer from the Ethereum Data Source
        let pointer_data = self
            .ethereum_source
            .next(block_ref, batcher_address)
            .await?;

        if pointer_data[2] != 0x0c {
            // check if there's more appropirate error, since we just fetched a celestia batch that does not correspond to celestia
            return Err(PipelineErrorKind::Temporary(PipelineError::EndOfSource));
        }

        let height_bytes = &pointer_data[3..11];
        let height = u64::from_le_bytes(height_bytes.try_into().unwrap());
        let hash_array: [u8; 32] = pointer_data[11..43]
            .try_into()
            .expect("Slice must be 32 bytes");
        let commitment = Commitment::new(hash_array);

        info!("Fetching blob at height: {:?}", height);
        let blob = self.celestia_source.next(height, commitment).await?;
        Ok(blob)
    }

    fn clear(&mut self) {
        self.celestia_source.clear();
        self.ethereum_source.clear();
    }
}
