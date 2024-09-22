//  Copyright 2024. The Tari Project
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

use log::*;
use tari_dan_common_types::PeerAddress;
use tari_dan_p2p::TariMessagingSpec;
use tari_epoch_manager::base_layer::EpochManagerHandle;
use tari_networking::NetworkingHandle;
use tokio::{sync::mpsc, task, task::JoinHandle};

use crate::p2p::services::consensus_gossip::{service::ConsensusGossipService, ConsensusGossipHandle};

const LOG_TARGET: &str = "tari::dan::validator_node::mempool";

pub fn spawn(
    epoch_manager: EpochManagerHandle<PeerAddress>,
    networking: NetworkingHandle<TariMessagingSpec>,
) -> (ConsensusGossipHandle, JoinHandle<anyhow::Result<()>>) {
    let (tx_consensus_request, rx_consensus_request) = mpsc::channel(10);

    let consensus_gossip = ConsensusGossipService::new(rx_consensus_request, epoch_manager, networking);
    let handle = ConsensusGossipHandle::new(tx_consensus_request);

    let join_handle = task::spawn(consensus_gossip.run());
    debug!(target: LOG_TARGET, "Spawning consensus gossip service (task: {:?})", join_handle);

    (handle, join_handle)
}
