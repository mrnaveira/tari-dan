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

use std::sync::Arc;

use async_trait::async_trait;
use tari_common_types::types::FixedHash;
use tari_dan_engine::{instruction::Transaction, instructions::Instruction};
use tokio::sync::Mutex;

use super::outbound::MempoolOutboundService;
use crate::{digital_assets_error::DigitalAssetError, models::TreeNodeHash};

#[async_trait]
pub trait MempoolService: Sync + Send + 'static {
    async fn submit_transaction(&mut self, transaction: &Transaction) -> Result<(), DigitalAssetError>;
    async fn read_block(&self, limit: usize) -> Result<Vec<Instruction>, DigitalAssetError>;
    async fn reserve_instruction_in_block(
        &mut self,
        instruction_hash: &FixedHash,
        block_hash: TreeNodeHash,
    ) -> Result<(), DigitalAssetError>;
    async fn remove_all_in_block(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError>;
    async fn release_reservations(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError>;
    async fn size(&self) -> usize;
}

pub struct ConcreteMempoolService {
    transactions: Vec<(Transaction, Option<TreeNodeHash>)>,
    instructions: Vec<(Instruction, Option<TreeNodeHash>)>,
    outbound_service: Option<Box<dyn MempoolOutboundService>>,
}

impl ConcreteMempoolService {
    pub fn new() -> Self {
        Self {
            transactions: vec![],
            instructions: vec![],
            outbound_service: None,
        }
    }
}

impl Default for ConcreteMempoolService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MempoolService for ConcreteMempoolService {
    async fn submit_transaction(&mut self, transaction: &Transaction) -> Result<(), DigitalAssetError> {
        // TODO: validate the transaction
        self.transactions.push((transaction.clone(), None));

        if let Some(outbound_service) = &mut self.outbound_service {
            outbound_service.propagate_transaction(transaction.clone()).await?;
        }

        Ok(())
    }

    async fn read_block(&self, limit: usize) -> Result<Vec<Instruction>, DigitalAssetError> {
        let mut result = vec![];
        for (i, (instruction, block_hash)) in self.instructions.iter().enumerate() {
            if i > limit {
                break;
            }
            if block_hash.is_none() {
                result.push(instruction.clone());
            }
        }
        Ok(result)
    }

    async fn reserve_instruction_in_block(
        &mut self,
        instruction_hash: &FixedHash,
        node_hash: TreeNodeHash,
    ) -> Result<(), DigitalAssetError> {
        for (instruction, node_hash_mut) in &mut self.instructions {
            if instruction.hash() == instruction_hash {
                *node_hash_mut = Some(node_hash);
                break;
            }
        }

        Ok(())
    }

    async fn remove_all_in_block(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError> {
        self.instructions = self
            .instructions
            .drain(..)
            .filter(|(_, node_hash)| node_hash.as_ref() != Some(block_hash))
            .collect();
        Ok(())
    }

    async fn release_reservations(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError> {
        for (_, block_hash_mut) in &mut self.instructions {
            if block_hash_mut.as_ref() == Some(block_hash) {
                *block_hash_mut = None;
            }
        }
        Ok(())
    }

    // async fn remove_instructions(&mut self, instructions: &[Instruction]) -> Result<(), DigitalAssetError> {
    //     let mut result = self.instructions.clone();
    //     for i in instructions {
    //         if let Some(position) = result.iter().position(|r| r == i) {
    //             result.remove(position);
    //         }
    //     }
    //     self.instructions = result;
    //     Ok(())
    // }

    async fn size(&self) -> usize {
        self.instructions
            .iter()
            .fold(0, |a, b| if b.1.is_none() { a + 1 } else { a })
    }
}

#[derive(Clone)]
pub struct MempoolServiceHandle {
    mempool: Arc<Mutex<ConcreteMempoolService>>,
}

impl MempoolServiceHandle {
    pub fn new() -> Self {
        let mempool_service = ConcreteMempoolService::new();

        Self {
            mempool: Arc::new(Mutex::new(mempool_service)),
        }
    }

    pub async fn set_outbound_service(&mut self, outbound_service: Box<dyn MempoolOutboundService>) {
        self.mempool.lock().await.outbound_service = Some(outbound_service);
    }
}

impl Default for MempoolServiceHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MempoolService for MempoolServiceHandle {
    async fn submit_transaction(&mut self, transaction: &Transaction) -> Result<(), DigitalAssetError> {
        self.mempool.lock().await.submit_transaction(transaction).await
    }

    async fn read_block(&self, limit: usize) -> Result<Vec<Instruction>, DigitalAssetError> {
        self.mempool.lock().await.read_block(limit).await
    }

    async fn reserve_instruction_in_block(
        &mut self,
        instruction_hash: &FixedHash,
        node_hash: TreeNodeHash,
    ) -> Result<(), DigitalAssetError> {
        self.mempool
            .lock()
            .await
            .reserve_instruction_in_block(instruction_hash, node_hash)
            .await
    }

    async fn remove_all_in_block(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError> {
        self.mempool.lock().await.remove_all_in_block(block_hash).await
    }

    async fn release_reservations(&mut self, block_hash: &TreeNodeHash) -> Result<(), DigitalAssetError> {
        self.mempool.lock().await.release_reservations(block_hash).await
    }

    async fn size(&self) -> usize {
        self.mempool.lock().await.size().await
    }
}