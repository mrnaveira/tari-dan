// Copyright 2023. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod bootstrap;
pub mod cli;
mod comms;
pub mod config;
mod dan_layer_scanner;
mod json_rpc;
mod p2p;
mod substate_manager;
mod substate_storage_sqlite;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use dan_layer_scanner::DanLayerScanner;
pub use json_rpc::GetSubstateRequest;
use log::*;
use substate_manager::SubstateManager;
use tari_app_utilities::identity_management::setup_node_identity;
use tari_common::{
    configuration::bootstrap::{grpc_default_port, ApplicationType},
    exit_codes::{ExitCode, ExitError},
};
use tari_comms::peer_manager::PeerFeatures;
use tari_dan_app_utilities::base_node_client::GrpcBaseNodeClient;
use tari_dan_core::{consensus_constants::ConsensusConstants, services::BaseNodeClient, storage::DbFactory};
use tari_dan_storage_sqlite::SqliteDbFactory;
use tari_shutdown::ShutdownSignal;
use tokio::{task, time};

use crate::{
    bootstrap::{spawn_services, Services},
    config::ApplicationConfig,
    json_rpc::{run_json_rpc, JsonRpcHandlers},
};

const LOG_TARGET: &str = "tari::indexer::app";
pub const DAN_PEER_FEATURES: PeerFeatures = PeerFeatures::COMMUNICATION_NODE;

pub async fn run_indexer(config: ApplicationConfig, mut shutdown_signal: ShutdownSignal) -> Result<(), ExitError> {
    let node_identity = setup_node_identity(
        &config.indexer.identity_file,
        config.indexer.public_address.as_ref(),
        true,
        DAN_PEER_FEATURES,
    )?;

    let db_factory = SqliteDbFactory::new(config.indexer.data_dir.clone());
    db_factory
        .migrate()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;
    let global_db = db_factory
        .get_or_create_global_db()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;

    let base_node_client = create_base_layer_clients(&config).await?;
    let services: Services = spawn_services(
        &config,
        shutdown_signal.clone(),
        node_identity.clone(),
        global_db,
        ConsensusConstants::devnet(), // TODO: change this eventually
    )
    .await?;

    let dan_layer_scanner = DanLayerScanner::new(
        services.epoch_manager.clone(),
        services.validator_node_client_factory.clone(),
        services.substate_store.clone(),
    );

    let substate_manager = Arc::new(SubstateManager::new(
        Arc::new(dan_layer_scanner),
        services.substate_store.clone(),
    ));

    // Run the JSON-RPC API
    if let Some(json_rpc_address) = config.indexer.json_rpc_address {
        info!(target: LOG_TARGET, "🌐 Started JSON-RPC server on {}", json_rpc_address);
        let handlers = JsonRpcHandlers::new(&services, base_node_client, substate_manager.clone());
        task::spawn(run_json_rpc(json_rpc_address, handlers));
    }

    let polling_shutdown = shutdown_signal.clone();
    tokio::spawn(async move {
        let result = run_substate_polling(&config, substate_manager, polling_shutdown).await;
        if let Err(err) = result {
            error!(target: LOG_TARGET, "run_substate_polling failed with error: {}", err);
        }
    });

    shutdown_signal.wait().await;

    Ok(())
}

async fn create_base_layer_clients(config: &ApplicationConfig) -> Result<GrpcBaseNodeClient, ExitError> {
    let mut base_node_client = GrpcBaseNodeClient::new(config.indexer.base_node_grpc_address.unwrap_or_else(|| {
        let port = grpc_default_port(ApplicationType::BaseNode, config.network);
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }));
    base_node_client
        .test_connection()
        .await
        .map_err(|error| ExitError::new(ExitCode::ConfigError, error))?;

    Ok(base_node_client)
}

async fn run_substate_polling(
    config: &ApplicationConfig,
    substate_manager: Arc<SubstateManager>,
    mut shutdown: ShutdownSignal,
) -> Result<(), anyhow::Error> {
    loop {
        tokio::select! {
            _ = time::sleep(config.indexer.dan_layer_scanning_internal) => {
                info!(target: LOG_TARGET, "Substate auto-scan initiated");
                match substate_manager.scan_and_update_substates().await {
                    Ok(_) => info!(target: LOG_TARGET, "Substate auto-scan succeded"),
                    Err(e) =>  error!(target: LOG_TARGET, "Substate auto-scan failed: {}", e),
                }
            },
            _ = shutdown.wait() => {
                dbg!("Shutting down run_substate_polling");
                break;
            },
        }
    }

    Ok(())
}