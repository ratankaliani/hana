//! This module contains all CLI-specific code for the single celestia chain entrypoint.

// Need to replicate single CLI since its not exposed / eported and can't wrap around it

use celestia_types::nmt::Namespace;
use clap::Parser;
use hana_oracle::hint::HintWrapper;
use kona_host::{
    cli::cli_styles,
    eth::http_provider,
    single::{SingleChainHost, SingleChainLocalInputs, SingleChainProviders},
    DiskKeyValueStore, MemoryKeyValueStore, OfflineHostBackend, OnlineHostBackend,
    OnlineHostBackendCfg, PreimageServer, SharedKeyValueStore, SplitKeyValueStore,
};
use maili_genesis::RollupConfig;
use serde::Serialize;

use alloy_primitives::{hex, Address};
use anyhow::{anyhow, Result};
use kona_preimage::{
    BidirectionalChannel, Channel, HintReader, HintWriter, OracleReader, OracleServer,
};
use kona_providers_alloy::{OnlineBeaconClient, OnlineBlobProvider};
use kona_std_fpvm::{FileChannel, FileDescriptor};
use op_alloy_network::Optimism;
use std::{str::FromStr, sync::Arc};
use tokio::{
    sync::RwLock,
    task::{self, JoinHandle},
};

use super::{CelestiaChainHintHandler, CelestiaChainProviders, OnlineCelestiaProvider};

/// The host binary CLI application arguments.
#[derive(Default, Parser, Serialize, Clone, Debug)]
#[command(styles = cli_styles())]
pub struct CelestiaChainHost {
    #[clap(flatten)]
    pub single_host: SingleChainHost,
    #[clap(flatten)]
    pub celestia_args: CelestiaCfg,
}

/// The host binary CLI application arguments.
#[derive(Default, Parser, Serialize, Clone, Debug)]
#[command(styles = cli_styles())]
pub struct CelestiaCfg {
    /// Connection to celestia network
    #[clap(long, alias = "celestia-conn", env)]
    pub celestia_connection: Option<String>,
    /// Token for the Celestia node connection
    #[clap(long, alias = "celestia-auth", env)]
    pub auth_token: Option<String>,
    /// Celestia Namespace to fetch data from
    #[clap(long, alias = "celestia-namespace", env)]
    pub namespace: Option<String>,
    /// Blobstream Address to check inclusion against
    #[clap(long, alias = "blobstream-address", env)]
    pub blobstream_address: Option<String>,
}

impl CelestiaChainHost {
    /// Starts the [SingleChainHost] application.
    pub async fn start(self) -> Result<()> {
        if self.single_host.server {
            let hint = FileChannel::new(FileDescriptor::HintRead, FileDescriptor::HintWrite);
            let preimage =
                FileChannel::new(FileDescriptor::PreimageRead, FileDescriptor::PreimageWrite);

            self.start_server(hint, preimage).await?.await?
        } else {
            self.start_native().await
        }
    }

    /// Starts the preimage server, communicating with the client over the provided channels.
    pub async fn start_server<C>(&self, hint: C, preimage: C) -> Result<JoinHandle<Result<()>>>
    where
        C: Channel + Send + Sync + 'static,
    {
        let kv_store = self.create_key_value_store()?;

        let task_handle = if self.is_offline() {
            task::spawn(
                PreimageServer::new(
                    OracleServer::new(preimage),
                    HintReader::new(hint),
                    Arc::new(OfflineHostBackend::new(kv_store)),
                )
                .start(),
            )
        } else {
            let providers = self.create_providers().await?;
            let backend = OnlineHostBackend::new(
                self.clone(),
                kv_store.clone(),
                providers,
                CelestiaChainHintHandler,
            );

            task::spawn(
                PreimageServer::new(
                    OracleServer::new(preimage),
                    HintReader::new(hint),
                    Arc::new(backend),
                )
                .start(),
            )
        };

        Ok(task_handle)
    }

    /// Starts the host in native mode, running both the client and preimage server in the same
    /// process.
    async fn start_native(&self) -> Result<()> {
        let hint = BidirectionalChannel::new()?;
        let preimage = BidirectionalChannel::new()?;

        let server_task = self.start_server(hint.host, preimage.host).await?;
        let client_task = task::spawn(hana_client::single::run(
            OracleReader::new(preimage.client),
            HintWriter::new(hint.client),
            None,
        ));

        let (_, client_result) = tokio::try_join!(server_task, client_task)?;

        // Bubble up the exit status of the client program if execution completes.
        std::process::exit(client_result.is_err() as i32)
    }

    /// Returns `true` if the host is running in offline mode.
    pub const fn is_offline(&self) -> bool {
        self.single_host.l1_node_address.is_none()
            && self.single_host.l2_node_address.is_none()
            && self.single_host.l1_beacon_address.is_none()
            && self.single_host.data_dir.is_some()
    }

    /// Reads the [RollupConfig] from the file system and returns it as a string.
    pub fn read_rollup_config(&self) -> Result<RollupConfig> {
        let path = self
            .single_host
            .rollup_config_path
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No rollup config path provided. Please provide a path to the rollup config."
                )
            })?;

        // Read the serialized config from the file system.
        let ser_config = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Error reading RollupConfig file: {e}"))?;

        // Deserialize the config and return it.
        serde_json::from_str(&ser_config)
            .map_err(|e| anyhow!("Error deserializing RollupConfig: {e}"))
    }

    /// Creates the key-value store for the host backend.
    fn create_key_value_store(&self) -> Result<SharedKeyValueStore> {
        let local_kv_store = SingleChainLocalInputs::new(self.single_host.clone());

        let kv_store: SharedKeyValueStore = if let Some(ref data_dir) = self.single_host.data_dir {
            let disk_kv_store = DiskKeyValueStore::new(data_dir.clone());
            let split_kv_store = SplitKeyValueStore::new(local_kv_store, disk_kv_store);
            Arc::new(RwLock::new(split_kv_store))
        } else {
            let mem_kv_store = MemoryKeyValueStore::new();
            let split_kv_store = SplitKeyValueStore::new(local_kv_store, mem_kv_store);
            Arc::new(RwLock::new(split_kv_store))
        };

        Ok(kv_store)
    }

    /// Creates the providers required for the host backend.
    /// TODO: Diego, Add Celestia Chain Provider
    async fn create_providers(&self) -> Result<CelestiaChainProviders> {
        let l1_provider = http_provider(
            self.single_host
                .l1_node_address
                .as_ref()
                .ok_or(anyhow!("Provider must be set"))?,
        );
        let blob_provider = OnlineBlobProvider::init(OnlineBeaconClient::new_http(
            self.single_host
                .l1_beacon_address
                .clone()
                .ok_or(anyhow!("Beacon API URL must be set"))?,
        ))
        .await;
        let l2_provider = http_provider::<Optimism>(
            self.single_host
                .l2_node_address
                .as_ref()
                .ok_or(anyhow!("L2 node address must be set"))?,
        );

        let celestia_client = celestia_rpc::Client::new(
            self.celestia_args
                .celestia_connection
                .as_ref()
                .ok_or(anyhow!("Celestia connection must be set"))?,
            Some(
                self.celestia_args
                    .auth_token
                    .as_ref()
                    .ok_or(anyhow!("Celestia auth token must be set"))?,
            ),
        )
        .await
        .expect("Failed creating rpc client");

        let namespace_bytes = hex::decode(
            &self
                .celestia_args
                .namespace
                .as_ref()
                .ok_or(anyhow!("Celestia Namespace must be set"))?,
        )
        .expect("Invalid hex");
        let namespace = Namespace::new_v0(&namespace_bytes).expect("Invalid namespace");

        let blobstream_address =
            Address::from_str(&self.celestia_args.blobstream_address.as_ref().unwrap())
                .expect("Invalid Blobstream Address");

        let celestia_provider =
            OnlineCelestiaProvider::new(celestia_client, namespace, blobstream_address);

        Ok(CelestiaChainProviders {
            inner_providers: SingleChainProviders {
                l1: l1_provider,
                blobs: blob_provider,
                l2: l2_provider,
            },
            celestia: celestia_provider,
        })
    }
}

impl OnlineHostBackendCfg for CelestiaChainHost {
    type HintType = HintWrapper;
    // TODO: Modify so that is uses "CelestiaChainProviders"
    type Providers = CelestiaChainProviders;
}
