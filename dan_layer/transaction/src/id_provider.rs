//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    sync::{atomic::AtomicU32, Arc, Mutex},
};

use tari_engine_types::hashing::{hasher, EngineHashDomainLabel};
use tari_template_lib::{
    models::{BucketId, ResourceAddress, TemplateAddress, VaultId},
    prelude::ComponentAddress,
    Hash,
};

use crate::Transaction;

#[derive(Debug, Clone)]
pub struct IdProvider {
    template_index_map: Arc<Mutex<HashMap<TemplateAddress, u64>>>,
    transaction_hash: Hash,
    max_ids: u32,
    current_id: Arc<AtomicU32>,
    bucket_id: Arc<AtomicU32>,
    uuid: Arc<AtomicU32>,
    last_random: Arc<Mutex<Hash>>,
}

#[derive(Debug, thiserror::Error)]
pub enum IdProviderError {
    #[error("Maximum ID allocation of {max} exceeded")]
    MaxIdsExceeded { max: u32 },
    #[error("Failed to acquire lock")]
    LockingError { operation: String },
}

impl IdProvider {
    pub fn new(transaction: Transaction, max_ids: u32) -> Self {
        let transaction_hash = *transaction.hash();
        let template_index_map = Arc::new(Mutex::new(generate_template_index_map(transaction)));
        Self {
            last_random: Arc::new(Mutex::new(transaction_hash)),
            template_index_map,
            transaction_hash,
            max_ids,
            // TODO: these should be ranges
            current_id: Arc::new(AtomicU32::new(0)),
            bucket_id: Arc::new(AtomicU32::new(1000)),
            uuid: Arc::new(AtomicU32::new(0)),
        }
    }

    fn next(&self) -> Result<u32, IdProviderError> {
        let id = self.current_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if id >= self.max_ids {
            return Err(IdProviderError::MaxIdsExceeded { max: self.max_ids });
        }
        Ok(id)
    }

    pub fn transaction_hash(&self) -> Hash {
        self.transaction_hash
    }

    /// Generates a new unique id H(tx_hash || n).
    /// NOTE: we rely on IDs being predictable for all outputs (components, resources, vaults).
    fn new_id(&self) -> Result<Hash, IdProviderError> {
        let id = generate_output_id(&self.transaction_hash, self.next()?);
        Ok(id)
    }

    pub fn new_component_address(
        &self,
        template_address: &TemplateAddress,
    ) -> Result<ComponentAddress, IdProviderError> {
        let mut template_index_map = self
            .template_index_map
            .lock()
            .map_err(|_| IdProviderError::LockingError {
                operation: "new_component_address".to_string(),
            })?;
        let index = match template_index_map.get(template_address) {
            Some(index) => *index,
            // for convenience, if no component address was specified in the outputs, we are going to default to index 0
            None => 0_u64,
        };
        template_index_map.insert(*template_address, index + 1);
        let component_address = ComponentAddress::new(*template_address, index);
        Ok(component_address)
    }

    pub fn new_resource_address(
        &self,
        template_address: &TemplateAddress,
        token_symbol: &str,
    ) -> Result<ResourceAddress, IdProviderError> {
        Ok(hasher(EngineHashDomainLabel::ResourceAddress)
            .chain(&template_address)
            .chain(&token_symbol)
            .result()
            .into())
    }

    pub fn new_address_hash(&self) -> Result<Hash, IdProviderError> {
        self.new_id()
    }

    pub fn new_vault_id(&self) -> Result<VaultId, IdProviderError> {
        Ok(self.new_id()?.into())
    }

    pub fn new_bucket_id(&self) -> BucketId {
        // Buckets are not saved to shards, so should not increment the hashes
        self.bucket_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn new_uuid(&self) -> Result<[u8; 32], IdProviderError> {
        let n = self.uuid.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = hasher(EngineHashDomainLabel::UuidOutput)
            .chain(&self.transaction_hash)
            .chain(&n)
            .result();
        Ok(id.into_array())
    }

    pub fn get_random_bytes(&self, len: u32) -> Result<Vec<u8>, IdProviderError> {
        let mut last_random = self.last_random.lock().map_err(|_| IdProviderError::LockingError {
            operation: "get_random_bytes".to_string(),
        })?;
        let mut result = Vec::with_capacity(len as usize);
        while result.len() < len as usize {
            let new_random = hasher(EngineHashDomainLabel::RandomBytes).chain(&*last_random).result();
            result.extend_from_slice(&new_random);
            *last_random = new_random;
        }
        if result.len() > len as usize {
            result.truncate(len as usize);
        }

        Ok(result)
    }
}

fn generate_output_id(hash: &Hash, n: u32) -> Hash {
    hasher(EngineHashDomainLabel::Output).chain(hash).chain(&n).result()
}

fn generate_template_index_map(transaction: Transaction) -> HashMap<TemplateAddress, u64> {
    let template_index_map: HashMap<_, _> = transaction
        .meta()
        .new_components_iter()
        .map(|c| (*c.template_address(), c.index()))
        .collect();
    template_index_map
}
#[cfg(test)]
mod tests {
    use tari_common_types::types::PrivateKey;

    use super::*;

    fn build_transaction() -> Transaction {
        Transaction::builder().sign(&PrivateKey::default()).build()
    }

    #[test]
    fn it_fails_if_generating_more_ids_than_the_max() {
        let tx = build_transaction();
        let id_provider = IdProvider::new(tx.clone(), 0);
        id_provider.new_id().unwrap_err();
        let id_provider = IdProvider::new(tx, 1);
        id_provider.new_id().unwrap();
        id_provider.new_id().unwrap_err();
    }
}
