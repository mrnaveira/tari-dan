//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{convert::TryInto, ops::RangeInclusive, sync::Arc};

use log::*;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_comms::{types::CommsPublicKey, NodeIdentity};
use tari_core::{
    blocks::BlockHeader,
    transactions::transaction_components::ValidatorNodeRegistration,
    ValidatorNodeBMT,
};
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_app_utilities::{base_node_client::GrpcBaseNodeClient, epoch_manager::EpochManagerEvent};
use tari_dan_common_types::{optional::Optional, vn_bmt_node_hash, Epoch, ShardId};
use tari_dan_core::{
    consensus_constants::{BaseLayerConsensusConstants, ConsensusConstants},
    models::{Committee, ValidatorNode},
    services::{
        epoch_manager::{EpochManagerError, ShardCommitteeAllocation},
        BaseNodeClient,
    },
};
use tari_dan_storage::global::{DbEpoch, DbValidatorNode, GlobalDb, MetadataKey};
use tari_dan_storage_sqlite::{global::SqliteGlobalDbAdapter, sqlite_shard_store_factory::SqliteShardStore};
use tokio::sync::broadcast;

use crate::p2p::services::{epoch_manager::PeerSyncManagerService, rpc_client::TariCommsValidatorNodeClientFactory};

const LOG_TARGET: &str = "tari::validator_node::epoch_manager::base_layer_epoch_manager";

#[derive(Clone)]
pub struct BaseLayerEpochManager {
    global_db: GlobalDb<SqliteGlobalDbAdapter>,
    shard_store: SqliteShardStore,
    pub base_node_client: GrpcBaseNodeClient,
    consensus_constants: ConsensusConstants,
    current_epoch: Epoch,
    current_block_height: u64,
    tx_events: broadcast::Sender<EpochManagerEvent>,
    node_identity: Arc<NodeIdentity>,
    validator_node_client_factory: TariCommsValidatorNodeClientFactory,
    current_shard_key: Option<ShardId>,
    base_layer_consensus_constants: Option<BaseLayerConsensusConstants>,
}

impl BaseLayerEpochManager {
    pub fn new(
        global_db: GlobalDb<SqliteGlobalDbAdapter>,
        shard_store: SqliteShardStore,
        base_node_client: GrpcBaseNodeClient,
        consensus_constants: ConsensusConstants,
        tx_events: broadcast::Sender<EpochManagerEvent>,
        node_identity: Arc<NodeIdentity>,
        validator_node_client_factory: TariCommsValidatorNodeClientFactory,
    ) -> Self {
        Self {
            global_db,
            shard_store,
            base_node_client,
            consensus_constants,
            current_epoch: Epoch(0),
            current_block_height: 0,
            tx_events,
            node_identity,
            validator_node_client_factory,
            current_shard_key: None,
            base_layer_consensus_constants: None,
        }
    }

    pub fn load_initial_state(&mut self) -> Result<(), EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);
        self.current_epoch = metadata
            .get_metadata(MetadataKey::EpochManagerCurrentEpoch)?
            .unwrap_or(Epoch(0));
        self.current_shard_key = metadata.get_metadata(MetadataKey::EpochManagerCurrentShardKey)?;
        self.base_layer_consensus_constants = metadata.get_metadata(MetadataKey::BaseLayerConsensusConstants)?;
        self.current_block_height = metadata
            .get_metadata(MetadataKey::EpochManagerCurrentBlockHeight)?
            .unwrap_or(0);

        Ok(())
    }

    pub async fn update_epoch(&mut self, block_height: u64, block_hash: FixedHash) -> Result<(), EpochManagerError> {
        let base_layer_constants = self.base_node_client.get_consensus_constants(block_height).await?;
        let epoch = base_layer_constants.height_to_epoch(block_height);
        self.update_current_block_height(block_height)?;
        if self.current_epoch >= epoch {
            // no need to update the epoch
            return Ok(());
        }

        info!(target: LOG_TARGET, "🌟 A new epoch {} is upon us", epoch);
        // extract and store in database the MMR of the epoch's validator nodes
        let epoch_header = self.base_node_client.get_header_by_hash(block_hash).await?;

        // persist the epoch data including the validator node set
        self.insert_current_epoch(epoch, epoch_header)?;
        self.update_base_layer_consensus_constants(base_layer_constants)?;

        self.tx_events
            .send(EpochManagerEvent::EpochChanged(epoch))
            .map_err(|_| EpochManagerError::SendError)?;

        Ok(())
    }

    pub async fn get_base_layer_consensus_constants(
        &mut self,
    ) -> Result<&BaseLayerConsensusConstants, EpochManagerError> {
        if let Some(ref constants) = self.base_layer_consensus_constants {
            return Ok(constants);
        }

        self.refresh_base_layer_consensus_constants().await?;

        Ok(self
            .base_layer_consensus_constants
            .as_ref()
            .expect("update_base_layer_consensus_constants did not set constants"))
    }

    async fn refresh_base_layer_consensus_constants(&mut self) -> Result<(), EpochManagerError> {
        let tip = self.base_node_client.get_tip_info().await?;
        let dan_tip = tip
            .height_of_longest_chain
            .saturating_sub(self.consensus_constants.base_layer_confirmations);

        let constants = self.base_node_client.get_consensus_constants(dan_tip).await?;
        self.update_base_layer_consensus_constants(constants)?;
        Ok(())
    }

    pub async fn add_validator_node_registration(
        &mut self,
        block_height: u64,
        registration: ValidatorNodeRegistration,
    ) -> Result<(), EpochManagerError> {
        let constants = self.get_base_layer_consensus_constants().await?;
        let next_epoch = constants.height_to_epoch(block_height) + Epoch(1);
        let next_epoch_height = constants.epoch_to_height(next_epoch);

        let shard_key = self
            .base_node_client
            .get_shard_key(next_epoch_height, registration.public_key())
            .await?
            .ok_or_else(|| EpochManagerError::ShardKeyNotFound {
                public_key: registration.public_key().clone(),
                block_height,
            })?;
        let new_vns = vec![DbValidatorNode {
            public_key: registration.public_key().to_vec(),
            shard_key: shard_key.as_bytes().to_vec(),
            epoch: next_epoch,
        }];

        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .validator_nodes(&mut tx)
            .insert_validator_nodes(new_vns)?;

        if registration.public_key() == self.node_identity.public_key() {
            let mut metadata = self.global_db.metadata(&mut tx);
            metadata.set_metadata(MetadataKey::EpochManagerCurrentShardKey, &shard_key)?;
            let last_registration_epoch = metadata
                .get_metadata::<Epoch>(MetadataKey::EpochManagerLastEpochRegistration)?
                .unwrap_or(Epoch(0));
            if last_registration_epoch < next_epoch {
                metadata.set_metadata(MetadataKey::EpochManagerLastEpochRegistration, &next_epoch)?;
            }
            self.current_shard_key = Some(shard_key);
            info!(
                target: LOG_TARGET,
                "📋️ This validator node is registered for epoch {}, shard key: {} ", next_epoch, shard_key
            );
        }

        tx.commit()?;

        Ok(())
    }

    fn insert_current_epoch(&mut self, epoch: Epoch, header: BlockHeader) -> Result<(), EpochManagerError> {
        let epoch_height = epoch.0;
        let db_epoch = DbEpoch {
            epoch: epoch_height,
            validator_node_mr: header.validator_node_mr.to_vec(),
        };

        let mut tx = self.global_db.create_transaction()?;

        self.global_db.epochs(&mut tx).insert_epoch(db_epoch)?;
        self.global_db
            .metadata(&mut tx)
            .set_metadata(MetadataKey::EpochManagerCurrentEpoch, &epoch)?;

        tx.commit()?;
        self.current_epoch = epoch;
        Ok(())
    }

    fn update_base_layer_consensus_constants(
        &mut self,
        base_layer_constants: BaseLayerConsensusConstants,
    ) -> Result<(), EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .metadata(&mut tx)
            .set_metadata(MetadataKey::BaseLayerConsensusConstants, &base_layer_constants)?;
        tx.commit()?;
        self.base_layer_consensus_constants = Some(base_layer_constants);
        Ok(())
    }

    fn update_current_block_height(&mut self, block_height: u64) -> Result<(), EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .metadata(&mut tx)
            .set_metadata(MetadataKey::EpochManagerCurrentBlockHeight, &block_height)?;
        tx.commit()?;
        self.current_block_height = block_height;
        Ok(())
    }

    pub fn current_epoch(&self) -> Epoch {
        self.current_epoch
    }

    pub fn current_block_height(&self) -> u64 {
        self.current_block_height
    }

    pub fn get_validator_shard_key(
        &self,
        epoch: Epoch,
        public_key: &PublicKey,
    ) -> Result<Option<ShardId>, EpochManagerError> {
        let (start_epoch, end_epoch) = self.get_epoch_range(epoch)?;
        let mut tx = self.global_db.create_transaction()?;
        let vn = self
            .global_db
            .validator_nodes(&mut tx)
            .get(start_epoch.as_u64(), end_epoch.as_u64(), public_key.as_bytes())
            .optional()?;

        Ok(vn.map(|vn| ShardId::from_bytes(&vn.shard_key).expect("Invalid Shard Key, Database is corrupt")))
    }

    pub fn last_registration_epoch(&self) -> Result<Option<Epoch>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        let mut metadata = self.global_db.metadata(&mut tx);
        let last_registration_epoch = metadata.get_metadata(MetadataKey::EpochManagerLastEpochRegistration)?;
        Ok(last_registration_epoch)
    }

    pub fn update_last_registration_epoch(&self, epoch: Epoch) -> Result<(), EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .metadata(&mut tx)
            .set_metadata(MetadataKey::EpochManagerLastEpochRegistration, &epoch)?;
        tx.commit()?;
        Ok(())
    }

    pub fn is_epoch_valid(&self, epoch: Epoch) -> bool {
        let current_epoch = self.current_epoch();
        current_epoch.0 <= epoch.0 + 10 && epoch.0 <= current_epoch.0 + 10
    }

    pub fn get_committees(
        &self,
        epoch: Epoch,
        shards: &[ShardId],
    ) -> Result<Vec<ShardCommitteeAllocation<CommsPublicKey>>, EpochManagerError> {
        let mut result = vec![];
        for &shard in shards {
            let committee = self.get_committee(epoch, shard)?;
            result.push(ShardCommitteeAllocation {
                shard_id: shard,
                committee,
            });
        }
        Ok(result)
    }

    pub fn get_committee_vns_from_shard_key(
        &self,
        epoch: Epoch,
        shard: ShardId,
    ) -> Result<Vec<ValidatorNode<CommsPublicKey>>, EpochManagerError> {
        // retrieve the validator nodes for this epoch from database
        let vns = self.get_validator_nodes_per_epoch(epoch)?;

        let half_committee_size = {
            let committee_size = self.consensus_constants.committee_size as usize;
            let v = committee_size / 2;
            if committee_size % 2 > 0 {
                v + 1
            } else {
                v
            }
        };
        if vns.len() < half_committee_size * 2 {
            return Ok(vns);
        }

        let mid_point = vns.iter().filter(|x| x.shard_key < shard).count();
        let begin =
            ((vns.len() as i64 + mid_point as i64 - (half_committee_size - 1) as i64) % vns.len() as i64) as usize;
        let end = ((mid_point as i64 + half_committee_size as i64) % vns.len() as i64) as usize;
        let mut result = Vec::with_capacity(half_committee_size * 2);
        if begin > mid_point {
            result.extend_from_slice(&vns[begin..]);
            result.extend_from_slice(&vns[0..mid_point]);
        } else {
            result.extend_from_slice(&vns[begin..mid_point]);
        }

        if end < mid_point {
            result.extend_from_slice(&vns[mid_point..]);
            result.extend_from_slice(&vns[0..end]);
        } else {
            result.extend_from_slice(&vns[mid_point..end]);
        }

        Ok(result)
    }

    pub fn get_committee(&self, epoch: Epoch, shard: ShardId) -> Result<Committee<CommsPublicKey>, EpochManagerError> {
        let result = self.get_committee_vns_from_shard_key(epoch, shard)?;
        Ok(Committee::new(result.into_iter().map(|v| v.public_key).collect()))
    }

    pub fn is_validator_in_committee(
        &self,
        epoch: Epoch,
        shard: ShardId,
        identity: CommsPublicKey,
    ) -> Result<bool, EpochManagerError> {
        // TODO: This can be made more efficient by searching an index for the specific identity
        let committee = self.get_committee(epoch, shard)?;
        Ok(committee.contains(&identity))
    }

    fn get_epoch_range(&self, end_epoch: Epoch) -> Result<(Epoch, Epoch), EpochManagerError> {
        let consensus_constants = self
            .base_layer_consensus_constants
            .as_ref()
            .ok_or(EpochManagerError::BaseLayerConsensusConstantsNotSet)?;

        let start_epoch = end_epoch.saturating_sub(consensus_constants.validator_node_registration_expiry());
        Ok((start_epoch, end_epoch))
    }

    pub fn get_validator_nodes_per_epoch(
        &self,
        epoch: Epoch,
    ) -> Result<Vec<ValidatorNode<CommsPublicKey>>, EpochManagerError> {
        let (start_epoch, end_epoch) = self.get_epoch_range(epoch)?;

        let mut tx = self.global_db.create_transaction()?;
        let db_vns = self
            .global_db
            .validator_nodes(&mut tx)
            .get_all_within_epochs(start_epoch.as_u64(), end_epoch.as_u64())?;
        let vns = db_vns
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()
            .expect("get_validator_nodes_per_epoch: Database is corrupt");
        Ok(vns)
    }

    pub fn filter_to_local_shards(
        &self,
        epoch: Epoch,
        for_addr: &CommsPublicKey,
        available_shards: &[ShardId],
    ) -> Result<Vec<ShardId>, EpochManagerError> {
        let mut result = vec![];
        for shard in available_shards {
            let committee = self.get_committee(epoch, *shard)?;
            if committee.contains(for_addr) {
                result.push(*shard);
            }
        }
        Ok(result)
    }

    pub fn get_validator_node_merkle_root(&self, epoch: Epoch) -> Result<Vec<u8>, EpochManagerError> {
        let mut tx = self.global_db.create_transaction()?;

        let query_res = self.global_db.epochs(&mut tx).get_epoch_data(epoch.0)?;

        match query_res {
            Some(db_epoch) => Ok(db_epoch.validator_node_mr),
            None => Err(EpochManagerError::NoEpochFound(epoch)),
        }
    }

    pub fn get_validator_node_bmt(&self, epoch: Epoch) -> Result<ValidatorNodeBMT, EpochManagerError> {
        let vns = self.get_validator_nodes_per_epoch(epoch)?;

        // TODO: the MMR struct should be serializable to store it only once and avoid recalculating it every time per
        // epoch
        let mut vn_bmt_vec = Vec::new();
        for vn in vns {
            vn_bmt_vec.push(vn_bmt_node_hash(&vn.public_key, &vn.shard_key).to_vec())
        }

        let vn_bmt = ValidatorNodeBMT::create(vn_bmt_vec);
        Ok(vn_bmt)
    }

    pub async fn on_scanning_complete(&mut self) -> Result<(), EpochManagerError> {
        {
            let mut tx = self.global_db.create_transaction()?;
            let last_sync_epoch = self
                .global_db
                .metadata(&mut tx)
                .get_metadata::<Epoch>(MetadataKey::EpochManagerLastSyncedEpoch)?;
            if last_sync_epoch.map(|e| e == self.current_epoch).unwrap_or(false) {
                info!(target: LOG_TARGET, "🌍️ Already synced for epoch {}", self.current_epoch);
                return Ok(());
            }
        }
        self.refresh_base_layer_consensus_constants().await?;
        let vn_shard_key = match self.get_validator_shard_key(self.current_epoch, self.node_identity.public_key())? {
            Some(vn_shard_key) => vn_shard_key,
            None => {
                info!(
                    target: LOG_TARGET,
                    "📋 Validator is not registered for epoch {}, Skipping state sync", self.current_epoch
                );
                return Ok(());
            },
        };

        // from current_shard_key we can get the corresponding vns committee
        let committee_size = self.consensus_constants.committee_size as usize;
        let committee_vns = self.get_committee_vns_from_shard_key(self.current_epoch, vn_shard_key)?;
        if committee_vns.is_empty() {
            return Err(EpochManagerError::NoCommitteeVns {
                epoch: self.current_epoch,
                shard_id: vn_shard_key,
            });
        }
        let (start_shard_id, end_shard_id) = get_committee_shard_range(committee_size, &committee_vns).into_inner();
        info!(
            target: LOG_TARGET,
            "♻ Starting state sync for epoch {} and shard range {} to {}",
            self.current_epoch,
            start_shard_id,
            end_shard_id
        );
        // synchronize state with committee validator nodes
        PeerSyncManagerService::new(self.validator_node_client_factory.clone(), self.shard_store.clone())
            .sync_peers_state(committee_vns, start_shard_id, end_shard_id, vn_shard_key)
            .await?;

        let mut tx = self.global_db.create_transaction()?;
        self.global_db
            .metadata(&mut tx)
            .set_metadata(MetadataKey::EpochManagerLastSyncedEpoch, &self.current_epoch)?;
        tx.commit()?;

        Ok(())
    }

    pub async fn remaining_registration_epochs(&mut self) -> Result<Option<Epoch>, EpochManagerError> {
        let last_registration_epoch = match self.last_registration_epoch()? {
            Some(epoch) => epoch,
            None => return Ok(None),
        };

        let constants = self.get_base_layer_consensus_constants().await?;
        let expiry = constants.validator_node_registration_expiry();

        let num_blocks_since_last_reg = self
            .current_epoch
            .checked_sub(last_registration_epoch)
            .expect("current epoch was less than the epoch we registered"); // Reorgs are not supported

        // None indicates that we are not registered, or a previous registration has expired
        Ok(expiry.checked_sub(num_blocks_since_last_reg))
    }
}

fn get_committee_shard_range<TAddr>(
    committee_size: usize,
    committee_vns: &[ValidatorNode<TAddr>],
) -> RangeInclusive<ShardId> {
    if committee_vns.len() < committee_size {
        let min_shard_id = ShardId::zero();
        let max_shard_id = ShardId([u8::MAX; 32]);
        min_shard_id..=max_shard_id
    } else {
        let min_shard_id = committee_vns
            .first()
            .expect("Committee VNs cannot be empty, at this point")
            .shard_key;
        let max_shard_id = committee_vns
            .last()
            .expect("Committee VNs cannot be empty, at this point")
            .shard_key;
        min_shard_id..=max_shard_id
    }
}
