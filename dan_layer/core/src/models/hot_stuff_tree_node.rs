// Copyright 2021. The Tari Project
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

use digest::{Digest, FixedOutput};
use serde::{Deserialize, Serialize};
use tari_crypto::hash::blake2::Blake256;
use tari_dan_common_types::{Epoch, PayloadId, ShardId};

use super::Payload;
use crate::{
    models::{NodeHeight, ObjectPledge, QuorumCertificate, TreeNodeHash},
    services::infrastructure_services::NodeAddressable,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HotStuffTreeNode<TAddr, TPayload> {
    hash: TreeNodeHash,
    parent: TreeNodeHash,
    shard: ShardId,
    height: NodeHeight,
    /// The payload that the node is proposing
    payload_id: PayloadId,
    payload: Option<TPayload>,
    /// How far in the consensus this payload is. It should be 4 in order to be committed.
    payload_height: NodeHeight,
    local_pledge: Option<ObjectPledge>,
    epoch: Epoch,
    justify: QuorumCertificate,
    // Mostly used for debugging
    proposed_by: TAddr,
}

impl<TAddr: NodeAddressable, TPayload: Payload> HotStuffTreeNode<TAddr, TPayload> {
    pub fn new(
        parent: TreeNodeHash,
        shard: ShardId,
        height: NodeHeight,
        payload_id: PayloadId,
        payload: Option<TPayload>,
        payload_height: NodeHeight,
        local_pledge: Option<ObjectPledge>,
        epoch: Epoch,
        proposed_by: TAddr,
        justify: QuorumCertificate,
    ) -> Self {
        let mut s = HotStuffTreeNode {
            hash: TreeNodeHash::zero(),
            parent,
            shard,
            payload_id,
            payload,
            epoch,
            height,
            justify,
            payload_height,
            local_pledge,
            proposed_by,
        };
        s.hash = s.calculate_hash();
        s
    }

    pub fn genesis() -> Self {
        let mut s = Self {
            parent: TreeNodeHash::zero(),
            payload_id: PayloadId::zero(),
            payload: None,
            payload_height: NodeHeight(0),
            hash: TreeNodeHash::zero(),
            shard: ShardId::zero(),
            height: NodeHeight(0),
            epoch: Epoch(0),
            proposed_by: TAddr::zero(),
            justify: QuorumCertificate::genesis(Epoch(0)),
            local_pledge: None,
        };
        s.hash = s.calculate_hash();
        s
    }

    pub fn calculate_hash(&self) -> TreeNodeHash {
        let result = Blake256::new()
            .chain(self.parent.as_bytes())
            .chain(self.epoch.to_le_bytes())
            .chain(self.height.to_le_bytes())
            .chain(self.justify.as_bytes())
            .chain(self.shard.to_le_bytes())
            .chain(self.payload_id.as_slice())
            .chain(self.payload_height.to_le_bytes())
            .chain(self.proposed_by.as_bytes());
        // TODO: Add in other fields
        // .chain((self.local_pledges.len() as u32).to_le_bytes())
        // .chain(self.local_pledges.iter().fold(Vec::new(), |mut acc, substate| {
        //     acc.extend_from_slice(substate.as_bytes())
        // }));

        result.finalize_fixed().into()
    }

    pub fn hash(&self) -> &TreeNodeHash {
        &self.hash
    }

    pub fn proposed_by(&self) -> &TAddr {
        &self.proposed_by
    }

    pub fn parent(&self) -> &TreeNodeHash {
        &self.parent
    }

    pub fn payload_id(&self) -> PayloadId {
        self.payload_id
    }

    pub fn payload(&self) -> Option<&TPayload> {
        self.payload.as_ref()
    }

    /// The payload height corresponds to the round number.
    pub fn payload_height(&self) -> NodeHeight {
        self.payload_height
    }

    /// The quorum certificate for this node
    pub fn justify(&self) -> &QuorumCertificate {
        &self.justify
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn shard(&self) -> ShardId {
        self.shard
    }

    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn local_pledge(&self) -> Option<&ObjectPledge> {
        self.local_pledge.as_ref()
    }
}

impl<TAddr: NodeAddressable, TPayload: Payload> PartialEq for HotStuffTreeNode<TAddr, TPayload> {
    fn eq(&self, other: &Self) -> bool {
        self.hash.eq(&other.hash)
    }
}
