//  Copyright 2021. The Tari Project
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

use std::marker::PhantomData;

use async_trait::async_trait;
use log::*;
use tari_common_types::types::FixedHash;
use tari_comms::types::CommsPublicKey;
use tari_comms_dht::{domain_message::OutboundDomainMessage, outbound::OutboundMessageRequester};
use tari_dan_core::{
    models::{HotStuffMessage, Payload, TariDanPayload},
    services::infrastructure_services::{NodeAddressable, OutboundService},
    DigitalAssetError,
};
use tari_p2p::tari_message::TariMessageType;
use tokio::sync::mpsc::Sender;

use crate::p2p::proto;

const LOG_TARGET: &str = "tari::validator_node::messages::outbound::validator_node";

pub struct TariCommsOutboundService<TPayload: Payload, TAddr: NodeAddressable> {
    outbound_message_requester: OutboundMessageRequester,
    loopback_service: Sender<(CommsPublicKey, HotStuffMessage<TPayload, TAddr>)>,
    contract_id: FixedHash,
    // TODO: Remove
    phantom: PhantomData<TPayload>,
}

impl<TPayload: Payload, TAddr: NodeAddressable> TariCommsOutboundService<TPayload, TAddr> {
    #[allow(dead_code)]
    pub fn new(
        outbound_message_requester: OutboundMessageRequester,
        loopback_service: Sender<(CommsPublicKey, HotStuffMessage<TPayload, TAddr>)>,
        contract_id: FixedHash,
    ) -> Self {
        Self {
            outbound_message_requester,
            loopback_service,
            contract_id,
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl OutboundService for TariCommsOutboundService<TariDanPayload, CommsPublicKey> {
    type Addr = CommsPublicKey;
    type Payload = TariDanPayload;

    async fn send(
        &mut self,
        from: CommsPublicKey,
        to: CommsPublicKey,
        message: HotStuffMessage<TariDanPayload, CommsPublicKey>,
    ) -> Result<(), DigitalAssetError> {
        debug!(target: LOG_TARGET, "Outbound message to be sent:{} {:?}", to, message);
        // Tari comms does allow sending to itself
        if from == to && *message.contract_id() == self.contract_id {
            debug!(
                target: LOG_TARGET,
                "Sending {:?} to self for contract {}",
                message.message_type(),
                message.contract_id()
            );
            self.loopback_service
                .send((from, message))
                .await
                .map_err(|_| DigitalAssetError::SendError {
                    context: "Sending to loopback".to_string(),
                })?;
            return Ok(());
        }

        let inner = proto::consensus::HotStuffMessage::from(message);
        let tari_message = OutboundDomainMessage::new(&TariMessageType::DanConsensusMessage, inner);

        self.outbound_message_requester.send_direct(to, tari_message).await?;
        Ok(())
    }

    async fn broadcast(
        &mut self,
        from: CommsPublicKey,
        committee: &[CommsPublicKey],
        message: HotStuffMessage<TariDanPayload, CommsPublicKey>,
    ) -> Result<(), DigitalAssetError> {
        for committee_member in committee {
            // TODO: send in parallel
            self.send(from.clone(), committee_member.clone(), message.clone())
                .await?;
        }
        Ok(())
    }
}
