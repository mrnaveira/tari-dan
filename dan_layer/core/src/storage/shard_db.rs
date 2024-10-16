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

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use tari_dan_common_types::{PayloadId, ShardId, SubstateChange, SubstateState};

use crate::{
    models::{
        vote_message::VoteMessage,
        HotStuffTreeNode,
        NodeHeight,
        ObjectPledge,
        Payload,
        QuorumCertificate,
        RecentTransaction,
        SQLTransaction,
        SubstateShardData,
        TreeNodeHash,
    },
    services::infrastructure_services::NodeAddressable,
    storage::shard_store::{ShardStoreTransaction, StoreError},
};

type PayloadVotes<TAddr, TPayload> =
    HashMap<PayloadId, HashMap<NodeHeight, HashMap<ShardId, HotStuffTreeNode<TAddr, TPayload>>>>;

// TODO: Clone is pretty bad here, this class should only be used for testing
#[derive(Debug, Default, Clone)]
pub struct MemoryShardDbInner<TAddr, TPayload> {
    current_state: HashMap<PayloadId, CurrentLeaderState>,
    // replica data
    shard_high_qcs: HashMap<ShardId, QuorumCertificate>,
    // pace maker data
    shard_leaf_nodes: HashMap<ShardId, (TreeNodeHash, NodeHeight)>,
    last_voted_heights: HashMap<ShardId, (NodeHeight, u32)>,
    lock_node_and_heights: HashMap<ShardId, (TreeNodeHash, NodeHeight)>,
    votes: HashMap<(TreeNodeHash, ShardId), Vec<(TAddr, VoteMessage)>>,
    nodes: HashMap<TreeNodeHash, HotStuffTreeNode<TAddr, TPayload>>,
    last_executed_height: HashMap<ShardId, NodeHeight>,
    payloads: HashMap<PayloadId, TPayload>,
    payload_votes: PayloadVotes<TAddr, TPayload>,
    objects: HashMap<ShardId, (SubstateState, Option<ObjectPledge>)>,
}

impl<TAddr: NodeAddressable, TPayload: Payload> MemoryShardDbInner<TAddr, TPayload> {
    pub fn new() -> Self {
        Self {
            current_state: HashMap::new(),
            shard_high_qcs: HashMap::new(),
            shard_leaf_nodes: HashMap::new(),
            last_voted_heights: HashMap::new(),
            lock_node_and_heights: HashMap::new(),
            votes: HashMap::new(),
            nodes: HashMap::new(),
            last_executed_height: HashMap::new(),
            payloads: HashMap::new(),
            payload_votes: HashMap::new(),
            objects: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryShardDb<TAddr, TPayload> {
    inner: Arc<RwLock<MemoryShardDbInner<TAddr, TPayload>>>,
    // TODO: use this to track state, pre-commit
    // current: MemoryShardDbInner<TAddr, TPayload>
}

impl<TAddr: NodeAddressable, TPayload: Payload> MemoryShardDb<TAddr, TPayload> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemoryShardDbInner::new())),
        }
    }
}

impl<TAddr: NodeAddressable, TPayload: Payload> ShardStoreTransaction<TAddr, TPayload>
    for MemoryShardDb<TAddr, TPayload>
{
    type Error = StoreError;

    fn get_high_qc_for(&self, shard: ShardId) -> Result<QuorumCertificate, Self::Error> {
        if let Some(qc) = self.inner.read().unwrap().shard_high_qcs.get(&shard) {
            Ok(qc.clone())
        } else {
            Ok(QuorumCertificate::genesis())
        }
    }

    fn update_high_qc(&mut self, shard: ShardId, qc: QuorumCertificate) -> Result<(), Self::Error> {
        let mut s = self.inner.write().unwrap();
        let entry = s.shard_high_qcs.entry(shard).or_insert_with(|| qc.clone());
        if qc.local_node_height() > entry.local_node_height() {
            *entry = qc.clone();
            s.shard_leaf_nodes
                .entry(qc.shard())
                .and_modify(|e| *e = (qc.local_node_hash(), qc.local_node_height()))
                .or_insert((qc.local_node_hash(), qc.local_node_height()));
        }
        Ok(())
    }

    fn get_leaf_node(&self, shard: ShardId) -> Result<(TreeNodeHash, NodeHeight), Self::Error> {
        Ok(
            if let Some(leaf) = self.inner.read().unwrap().shard_leaf_nodes.get(&shard) {
                *leaf
            } else {
                (TreeNodeHash::zero(), NodeHeight(0))
            },
        )
    }

    fn update_leaf_node(&mut self, shard: ShardId, node: TreeNodeHash, height: NodeHeight) -> Result<(), StoreError> {
        let mut guard = self.inner.write().unwrap();
        let leaf = guard.shard_leaf_nodes.entry(shard).or_insert((node, height));
        *leaf = (node, height);
        Ok(())
    }

    fn get_last_voted_height(&self, shard: ShardId) -> Result<(NodeHeight, u32), Self::Error> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .last_voted_heights
            .get(&shard)
            .copied()
            .unwrap_or((NodeHeight(0), 0)))
    }

    fn set_last_voted_height(
        &mut self,
        shard: ShardId,
        height: NodeHeight,
        leader_round: u32,
    ) -> Result<(), Self::Error> {
        let mut guard = self.inner.write().unwrap();
        let entry = guard.last_voted_heights.entry(shard).or_insert(height);
        *entry = height;
        Ok(())
    }

    fn get_locked_node_hash_and_height(&self, shard: ShardId) -> Result<(TreeNodeHash, NodeHeight), Self::Error> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .lock_node_and_heights
            .get(&shard)
            .copied()
            .unwrap_or((TreeNodeHash::zero(), NodeHeight(0))))
    }

    fn set_locked(
        &mut self,
        shard: ShardId,
        node_hash: TreeNodeHash,
        node_height: NodeHeight,
    ) -> Result<(), Self::Error> {
        self.inner
            .write()
            .unwrap()
            .lock_node_and_heights
            .entry(shard)
            .and_modify(|e| *e = (node_hash, node_height));
        Ok(())
    }

    fn has_vote_for(&self, from: &TAddr, node_hash: TreeNodeHash, shard: ShardId) -> Result<bool, Self::Error> {
        Ok(
            if let Some(sigs) = self.inner.read().unwrap().votes.get(&(node_hash, shard)) {
                sigs.iter().any(|(f, _)| f == from)
            } else {
                false
            },
        )
    }

    fn save_received_vote_for(
        &mut self,
        from: TAddr,
        node_hash: TreeNodeHash,
        shard: ShardId,
        vote_message: VoteMessage,
    ) -> Result<usize, Self::Error> {
        let mut guard = self.inner.write().unwrap();
        let entry = guard.votes.entry((node_hash, shard)).or_insert(vec![]);
        entry.push((from, vote_message));
        Ok(entry.len())
    }

    fn get_received_votes_for(&self, node_hash: TreeNodeHash, shard: ShardId) -> Result<Vec<VoteMessage>, Self::Error> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .votes
            .get(&(node_hash, shard))
            .map(|v| v.iter().map(|s| s.1.clone()).collect())
            .unwrap_or_default())
    }

    fn save_leader_proposals(
        &mut self,
        shard: ShardId,
        payload: PayloadId,
        payload_height: NodeHeight,
        leader_round: u32,
        node: HotStuffTreeNode<TAddr, TPayload>,
    ) -> Result<(), Self::Error> {
        let mut guard = self.inner.write().unwrap();
        let payload_entry = guard.payload_votes.entry(payload).or_insert_with(HashMap::new);
        let height_entry = payload_entry.entry(payload_height).or_insert_with(HashMap::new);
        let leader_round_entry = payload_entry.entry(leader_round).or_insert_with(HashMap::new);
        height_entry.insert(shard, node);
        Ok(())
    }

    fn get_leader_proposals(
        &self,
        payload: PayloadId,
        payload_height: NodeHeight,
        shard: ShardId,
    ) -> Result<Option<HotStuffTreeNode<TAddr, TPayload>>, Self::Error> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .payload_votes
            .get(&payload)
            .and_then(|pv| pv.get(&payload_height))
            .and_then(|ph| ph.get(&shard).cloned()))
    }

    fn save_node(&mut self, node: HotStuffTreeNode<TAddr, TPayload>) -> Result<(), Self::Error> {
        self.inner.write().unwrap().nodes.insert(*node.hash(), node);
        Ok(())
    }

    fn get_node(&self, node_hash: &TreeNodeHash) -> Result<HotStuffTreeNode<TAddr, TPayload>, Self::Error> {
        if node_hash == &TreeNodeHash::zero() {
            Ok(HotStuffTreeNode::genesis())
        } else {
            self.inner
                .read()
                .unwrap()
                .nodes
                .get(node_hash)
                .cloned()
                .ok_or(StoreError::NodeNotFound)
        }
    }

    fn set_last_executed_height(&mut self, shard: ShardId, height: NodeHeight) -> Result<(), Self::Error> {
        self.inner
            .write()
            .unwrap()
            .last_executed_height
            .entry(shard)
            .and_modify(|e| *e = height);
        Ok(())
    }

    fn get_last_executed_height(&self, shard: ShardId) -> Result<NodeHeight, Self::Error> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .last_executed_height
            .get(&shard)
            .copied()
            .unwrap_or(NodeHeight(0)))
    }

    fn get_payload(&self, payload_id: &PayloadId) -> Result<TPayload, Self::Error> {
        self.inner
            .read()
            .unwrap()
            .payloads
            .get(payload_id)
            .cloned()
            .ok_or(StoreError::CannotFindPayload)
    }

    fn set_payload(&mut self, payload: TPayload) -> Result<(), Self::Error> {
        let payload_id = payload.to_id();
        self.inner
            .write()
            .unwrap()
            .payloads
            .entry(payload_id)
            .or_insert(payload);
        Ok(())
    }

    fn pledge_object(
        &mut self,
        shard: ShardId,
        payload: PayloadId,
        _change: SubstateChange,
        current_height: NodeHeight,
    ) -> Result<ObjectPledge, Self::Error> {
        let mut guard = self.inner.write().unwrap();
        let entry = guard
            .objects
            .entry(shard)
            .or_insert((SubstateState::DoesNotExist, None));
        if let Some(existing_pledge) = &entry.1 {
            if existing_pledge.pledged_until > current_height {
                return Ok(existing_pledge.clone());
            }
        }

        let pledge = ObjectPledge {
            shard_id: shard,
            current_state: entry.0.clone(),
            pledged_to_payload: payload,
            pledged_until: current_height + NodeHeight(4),
        };
        entry.1 = Some(pledge.clone());
        Ok(pledge)
    }

    fn commit(&mut self) -> Result<(), Self::Error> {
        // TODO: this is not currently atomic across multiple operations and rollbacks are not supported.
        //       We could track local state changes in a separate non-shared state instance, and apply the changes to
        //       the shared state here on commit.
        Ok(())
    }

    fn save_substate_changes(
        &mut self,
        _changes: &HashMap<ShardId, SubstateState>,
        _node: &HotStuffTreeNode<TAddr, TPayload>,
    ) -> Result<(), Self::Error> {
        // todo!()
        Ok(())
    }

    fn insert_substates(&mut self, _substate_data: SubstateShardData) -> Result<(), Self::Error> {
        // todo!()
        Ok(())
    }

    fn get_state_inventory(&self, _start_shard: ShardId, _end_shard: ShardId) -> Result<Vec<ShardId>, Self::Error> {
        Ok(vec![])
    }

    fn get_substate_states(&self, _shards: &[ShardId]) -> Result<Vec<SubstateShardData>, Self::Error> {
        // todo!()
        Ok(vec![])
    }

    fn get_recent_transactions(&self) -> Result<Vec<RecentTransaction>, Self::Error> {
        // todo!()
        Ok(vec![])
    }

    fn get_transaction(&self, _payload_id: Vec<u8>) -> Result<Vec<SQLTransaction>, Self::Error> {
        Ok(vec![])
    }
}
