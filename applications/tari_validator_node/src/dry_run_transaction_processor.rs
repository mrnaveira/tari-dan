use std::collections::HashMap;

use tari_dan_common_types::{ObjectPledge, ShardId};
use tari_dan_core::{
    models::{Payload, TariDanPayload},
    services::{epoch_manager::EpochManager, PayloadProcessor, PayloadProcessorError},
    storage::{
        shard_store::{ShardStore, ShardStoreTransaction},
        StorageError,
    },
};
use tari_dan_engine::transaction::Transaction;
use tari_dan_storage_sqlite::sqlite_shard_store_factory::SqliteShardStore;
use tari_engine_types::commit_result::FinalizeResult;
use thiserror::Error;

use crate::{
    p2p::services::{epoch_manager::handle::EpochManagerHandle, template_manager::TemplateManager},
    payload_processor::TariDanPayloadProcessor,
};

#[derive(Error, Debug)]
pub enum DryRunTransactionProcessorError {
    #[error("PayloadProcessor error: {0}")]
    PayloadProcessorError(#[from] PayloadProcessorError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("No substate found for shard id {shard_id}")]
    SubstateNotFound { shard_id: ShardId },
}

#[derive(Clone)]
pub struct DryRunTransactionProcessor {
    /// The epoch manager
    epoch_manager: EpochManagerHandle,
    /// The payload processor. This determines whether a payload proposal results in an accepted or rejected vote.
    payload_processor: TariDanPayloadProcessor<TemplateManager>,
    /// Store used to persist consensus state.
    shard_store: SqliteShardStore,
}

impl DryRunTransactionProcessor {
    pub fn new(
        epoch_manager: EpochManagerHandle,
        payload_processor: TariDanPayloadProcessor<TemplateManager>,
        shard_store: SqliteShardStore,
    ) -> Self {
        Self {
            epoch_manager,
            payload_processor,
            shard_store,
        }
    }

    // TODO: create a error enum
    pub async fn process_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<FinalizeResult, DryRunTransactionProcessorError> {
        // get the list of involved shards for the transaction
        let payload = TariDanPayload::new(transaction.clone());
        let involved_shards = payload.involved_shards();

        // get the local shard state
        let _epoch = self.epoch_manager.current_epoch().await.map_err(|e| e.to_string());

        // get the pledges for all local shards
        let shard_pledges = self.get_local_pledges(involved_shards).await?;

        // TODO: get non local shard pledges

        let result = self.payload_processor.process_payload(payload, shard_pledges)?;
        Ok(result)
    }

    async fn get_local_pledges(
        &self,
        involved_shards: Vec<ShardId>,
    ) -> Result<HashMap<ShardId, Option<ObjectPledge>>, DryRunTransactionProcessorError> {
        dbg!(&involved_shards);
        let tx = self.shard_store.create_tx().unwrap();
        let inventory = tx.get_state_inventory().unwrap();
        dbg!(&inventory);

        let local_shard_ids: Vec<ShardId> = involved_shards.into_iter().filter(|s| inventory.contains(s)).collect();
        dbg!(&local_shard_ids);
        let mut local_pledges = HashMap::new();
        // TODO: create a DB method to get the substates of a list of shards in a single transaction
        for shard_id in local_shard_ids {
            let substate_data = tx
                .get_substate_states(shard_id, shard_id, &[])?
                .first()
                .ok_or(DryRunTransactionProcessorError::SubstateNotFound { shard_id })?
                .clone();
            dbg!(&substate_data);
            let local_pledge = ObjectPledge {
                shard_id,
                current_state: substate_data.substate().clone(),
                pledged_to_payload: substate_data.payload_id(),
                pledged_until: substate_data.height(),
            };
            local_pledges.insert(shard_id, Some(local_pledge));
        }
        dbg!(&local_pledges);
        Ok(local_pledges)
    }
}