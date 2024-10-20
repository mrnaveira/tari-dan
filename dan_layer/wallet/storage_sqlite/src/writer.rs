//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::MutexGuard,
};

use chrono::NaiveDateTime;
use diesel::{
    sql_query,
    sql_types::{BigInt, Bool, Integer, Nullable, Text},
    OptionalExtension,
    QueryDsl,
    RunQueryDsl,
    SqliteConnection,
};
use log::*;
use serde::Serialize;
use tari_common_types::types::{Commitment, FixedHash, PublicKey};
use tari_dan_common_types::QuorumCertificate;
use tari_dan_wallet_sdk::{
    models::{
        ConfidentialOutputModel,
        ConfidentialProofId,
        OutputStatus,
        SubstateModel,
        TransactionStatus,
        VaultModel,
        VersionedSubstateAddress,
    },
    storage::{WalletStorageError, WalletStoreReader, WalletStoreWriter},
};
use tari_engine_types::{commit_result::FinalizeResult, substate::SubstateAddress, TemplateAddress};
use tari_template_lib::models::Amount;
use tari_transaction::Transaction;
use tari_utilities::hex::Hex;

use crate::{diesel::ExpressionMethods, models, reader::ReadTransaction, serialization::serialize_json};

const LOG_TARGET: &str = "auth::tari::dan::wallet_sdk::storage_sqlite::writer";

pub struct WriteTransaction<'a> {
    /// In SQLite any transaction is writable. We keep a ReadTransaction to satisfy the Deref requirement of the
    /// WalletStore.
    transaction: ReadTransaction<'a>,
}

impl<'a> WriteTransaction<'a> {
    pub fn new(connection: MutexGuard<'a, SqliteConnection>) -> Self {
        Self {
            transaction: ReadTransaction::new(connection),
        }
    }
}

impl WalletStoreWriter for WriteTransaction<'_> {
    fn commit(mut self) -> Result<(), WalletStorageError> {
        self.transaction.commit()?;
        Ok(())
    }

    fn rollback(mut self) -> Result<(), WalletStorageError> {
        self.transaction.rollback()?;
        Ok(())
    }

    // -------------------------------- KeyManager -------------------------------- //

    fn key_manager_insert(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        use crate::schema::key_manager_states;
        let index =
            i64::try_from(index).map_err(|_| WalletStorageError::general("key_manager_insert", "index is negative"))?;
        let count = key_manager_states::table
            .select(key_manager_states::id)
            .filter(key_manager_states::branch_seed.eq(branch))
            .limit(1)
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_insert", e))?;

        // Set active if this is the only key branch
        let is_active = count == 0;

        let value_set = (
            key_manager_states::branch_seed.eq(branch),
            key_manager_states::index.eq(index),
            key_manager_states::is_active.eq(is_active),
        );

        diesel::insert_into(key_manager_states::table)
            .values(value_set)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_insert", e))?;

        Ok(())
    }

    fn key_manager_set_active_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        use crate::schema::key_manager_states;
        let index = i64::try_from(index)
            .map_err(|_| WalletStorageError::general("key_manager_set_active_index", "index too large"))?;

        let active_id = key_manager_states::table
            .select(key_manager_states::id)
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::index.eq(index))
            .limit(1)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "key_manager_set_active_index",
                entity: "key_manager_states".to_string(),
                key: format!("branch = {}, index = {}", branch, index),
            })?;

        diesel::update(key_manager_states::table)
            .set((
                key_manager_states::is_active.eq(false),
                key_manager_states::updated_at.eq(diesel::dsl::now),
            ))
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::is_active.eq(true))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?;

        diesel::update(key_manager_states::table)
            .set((
                key_manager_states::is_active.eq(true),
                key_manager_states::updated_at.eq(diesel::dsl::now),
            ))
            .filter(key_manager_states::id.eq(active_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?;

        Ok(())
    }

    // -------------------------------- Config -------------------------------- //

    fn config_set<T: Serialize>(&mut self, key: &str, value: &T, is_encrypted: bool) -> Result<(), WalletStorageError> {
        use crate::schema::config;

        let exists = config::table
            .filter(config::key.eq(key))
            .limit(1)
            .count()
            .get_result(self.connection())
            .map(|count: i64| count > 0)
            .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;

        if exists {
            sql_query("UPDATE config SET value = ?, is_encrypted = ?, updated_at = CURRENT_TIMESTAMP WHERE key = ?")
                .bind::<Text, _>(serialize_json(value)?)
                .bind::<Text, _>(key)
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;
        } else {
            sql_query("INSERT INTO config (key, value, is_encrypted) VALUES (?, ?, ?)")
                .bind::<Text, _>(key)
                .bind::<Text, _>(serialize_json(value)?)
                .bind::<Bool, _>(is_encrypted)
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;
        }

        Ok(())
    }

    // -------------------------------- Transactions -------------------------------- //

    fn transactions_insert(&mut self, transaction: &Transaction, is_dry_run: bool) -> Result<(), WalletStorageError> {
        let status = if is_dry_run {
            TransactionStatus::DryRun
        } else {
            TransactionStatus::New
        };

        sql_query(
            "INSERT INTO transactions (hash, instructions, sender_address, fee, signature, meta, status, dry_run) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind::<Text, _>(transaction.hash().to_string())
        .bind::<Text, _>(serialize_json(transaction.instructions())?)
        .bind::<Text, _>(transaction.sender_public_key().to_hex())
        .bind::<BigInt, _>(transaction.fee() as i64)
        .bind::<Text, _>(serialize_json(transaction.signature())?)
        .bind::<Text, _>(serialize_json(transaction.meta())?)
        .bind::<Text, _>(status.as_key_str())
        .bind::<Bool, _>(is_dry_run)
        .execute(self.connection())
        .map_err(|e| WalletStorageError::general("transactions_insert", e))?;

        Ok(())
    }

    fn transactions_set_result_and_status(
        &mut self,
        hash: FixedHash,
        result: Option<&FinalizeResult>,
        qcs: Option<&[QuorumCertificate]>,
        new_status: TransactionStatus,
    ) -> Result<(), WalletStorageError> {
        let num_rows = sql_query(
            "UPDATE transactions SET result = ?, status = ?, qcs = ?, updated_at = CURRENT_TIMESTAMP WHERE hash = ?",
        )
        .bind::<Nullable<Text>, _>(result.map(serialize_json).transpose()?)
        .bind::<Text, _>(new_status.as_key_str())
        .bind::<Nullable<Text>, _>(qcs.map(serialize_json).transpose()?)
        .bind::<Text, _>(hash.to_string())
        .execute(self.connection())
        .map_err(|e| WalletStorageError::general("transactions_set_result_and_status", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "transactions_set_result_and_status",
                entity: "transaction".to_string(),
                key: hash.to_string(),
            });
        }

        Ok(())
    }

    // -------------------------------- Substates -------------------------------- //

    fn substates_insert_parent(
        &mut self,
        tx_hash: FixedHash,
        substate: VersionedSubstateAddress,
        module_name: String,
        template_addr: TemplateAddress,
    ) -> Result<(), WalletStorageError> {
        sql_query(
            "INSERT INTO substates (module_name, address, parent_address, transaction_hash, template_address, \
             version) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind::<Nullable<Text>, _>(Some(module_name))
        .bind::<Text, _>(substate.address.to_string())
        .bind::<Nullable<Text>, _>(None::<String>)
        .bind::<Text, _>(tx_hash.to_string())
        .bind::<Nullable<Text>, _>(Some(template_addr.to_string()))
        .bind::<Integer, _>(substate.version as i32)
        .execute(self.connection())
        .map_err(|e| WalletStorageError::general("substates_insert", e))?;

        Ok(())
    }

    fn substates_insert_child(
        &mut self,
        tx_hash: FixedHash,
        parent: SubstateAddress,
        child: VersionedSubstateAddress,
    ) -> Result<(), WalletStorageError> {
        sql_query("INSERT INTO substates (transaction_hash, address, parent_address, version) VALUES (?, ?, ?, ?)")
            .bind::<Text, _>(tx_hash.to_string())
            .bind::<Text, _>(child.address.to_string())
            .bind::<Nullable<Text>, _>(Some(parent.to_string()))
            .bind::<Integer, _>(child.version as i32)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("substates_insert", e))?;
        Ok(())
    }

    fn substates_remove(
        &mut self,
        substate_addr: &VersionedSubstateAddress,
    ) -> Result<SubstateModel, WalletStorageError> {
        let substate = self.transaction.substates_get(&substate_addr.address)?;
        let num_rows = sql_query("DELETE FROM substates WHERE address = ? AND version = ?")
            .bind::<Text, _>(substate_addr.address.to_string())
            .bind::<Integer, _>(substate_addr.version as i32)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("substates_remove", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "substates_remove",
                entity: "substate".to_string(),
                key: substate.address.to_string(),
            });
        }

        Ok(substate)
    }

    // -------------------------------- Accounts -------------------------------- //

    fn accounts_insert(
        &mut self,
        account_name: &str,
        address: &SubstateAddress,
        owner_key_index: u64,
    ) -> Result<(), WalletStorageError> {
        sql_query("INSERT INTO accounts (name, address, owner_key_index) VALUES (?, ?, ?)")
            .bind::<Text, _>(account_name)
            .bind::<Text, _>(address.to_string())
            .bind::<BigInt, _>(owner_key_index as i64)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_insert", e))?;

        Ok(())
    }

    fn accounts_update(&mut self, address: &SubstateAddress, new_name: Option<&str>) -> Result<(), WalletStorageError> {
        use crate::schema::accounts;

        let changeset = (new_name.map(|n| accounts::name.eq(n)),);

        let num_rows = diesel::update(accounts::table)
            .set(changeset)
            .filter(accounts::address.eq(address.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_update", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "accounts_update",
                entity: "account".to_string(),
                key: address.to_string(),
            });
        }

        Ok(())
    }

    fn vaults_insert(&mut self, vault: VaultModel) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, vaults};

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(vault.account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_insert", e))?;

        let values = (
            vaults::account_id.eq(account_id),
            vaults::address.eq(vault.address.to_string()),
            vaults::balance.eq(vault.balance.value()),
            vaults::resource_address.eq(vault.resource_address.to_string()),
            vaults::resource_type.eq(format!("{:?}", vault.resource_type)),
            vaults::token_symbol.eq(vault.token_symbol),
        );
        diesel::insert_into(vaults::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_insert", e))?;

        Ok(())
    }

    fn vaults_update(
        &mut self,
        vault_address: &SubstateAddress,
        balance: Option<Amount>,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::vaults;

        let Some(balance) = balance else {
            return Ok(());
        };

        let changeset = vaults::balance.eq(balance.value());

        let num_rows = diesel::update(vaults::table)
            .set(changeset)
            .filter(vaults::address.eq(vault_address.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_update", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_update",
                entity: "vault".to_string(),
                key: vault_address.to_string(),
            });
        }

        Ok(())
    }

    // -------------------------------- Outputs -------------------------------- //

    fn outputs_lock_smallest_amount(
        &mut self,
        vault_address: &SubstateAddress,
        locked_by_proof: ConfidentialProofId,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        use crate::schema::{accounts, outputs, vaults};

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let locked_output = outputs::table
            .filter(outputs::vault_id.eq(vault_id))
            .filter(outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .order_by(outputs::value.asc())
            .first::<models::ConfidentialOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let locked_output = locked_output.ok_or_else(|| WalletStorageError::NotFound {
            operation: "outputs_lock_smallest_amount",
            entity: "output".to_string(),
            key: format!("vault={}, locked_by_proof={}", vault_address, locked_by_proof),
        })?;

        let account_address = accounts::table
            .select(accounts::address)
            .filter(accounts::id.eq(locked_output.account_id))
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let changeset = (
            outputs::status.eq(OutputStatus::Locked.as_key_str()),
            outputs::locked_by_proof.eq(locked_by_proof as i32),
            outputs::locked_at.eq(diesel::dsl::now),
        );
        diesel::update(outputs::table)
            .set(changeset)
            .filter(outputs::id.eq(locked_output.id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        Ok(ConfidentialOutputModel {
            account_address: SubstateAddress::from_str(&account_address).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "account address",
                    details: e.to_string(),
                }
            })?,
            vault_address: vault_address.clone(),
            commitment: Commitment::from_hex(&locked_output.commitment).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            value: locked_output.value as u64,
            sender_public_nonce: locked_output
                .sender_public_nonce
                .map(|nonce| PublicKey::from_hex(&nonce).unwrap()),
            secret_key_index: locked_output.secret_key_index as u64,
            public_asset_tag: None,
            status: OutputStatus::Locked,
            locked_by_proof: Some(locked_by_proof),
        })
    }

    fn outputs_insert(&mut self, output: ConfidentialOutputModel) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, outputs, vaults};

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(&output.account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(&output.vault_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        diesel::insert_into(outputs::table)
            .values((
                outputs::account_id.eq(account_id),
                outputs::vault_id.eq(vault_id),
                outputs::commitment.eq(output.commitment.to_hex()),
                outputs::value.eq(output.value as i64),
                outputs::sender_public_nonce.eq(output.sender_public_nonce.map(|pk| pk.to_hex())),
                outputs::secret_key_index.eq(output.secret_key_index as i64),
                outputs::status.eq(output.status.as_key_str()),
                outputs::locked_by_proof.eq(output.locked_by_proof.map(|v| v as i32)),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        Ok(())
    }

    fn outputs_finalize_by_proof_id(&mut self, proof_id: ConfidentialProofId) -> Result<(), WalletStorageError> {
        use crate::schema::outputs;

        // Unlock locked unconfirmed outputs
        diesel::update(outputs::table)
            .filter(outputs::locked_by_proof.eq(proof_id as i32))
            .filter(outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                outputs::locked_by_proof.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<NaiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        // Mark locked outputs as spent
        diesel::update(outputs::table)
            .filter(outputs::locked_by_proof.eq(proof_id as i32))
            .filter(outputs::status.eq(OutputStatus::Locked.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Spent.as_key_str()),
                outputs::locked_by_proof.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<NaiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        Ok(())
    }

    fn outputs_release_by_proof_id(&mut self, proof_id: ConfidentialProofId) -> Result<(), WalletStorageError> {
        use crate::schema::outputs;

        // Unlock locked unspent outputs
        diesel::update(outputs::table)
            .filter(outputs::locked_by_proof.eq(proof_id as i32))
            .filter(outputs::status.eq(OutputStatus::Locked.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                outputs::locked_by_proof.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<NaiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        // Remove outputs that were created by this proof
        diesel::delete(outputs::table)
            .filter(outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .filter(outputs::locked_by_proof.eq(proof_id as i32))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        Ok(())
    }

    // Proofs
    fn proofs_insert(&mut self, vault_address: &SubstateAddress) -> Result<ConfidentialProofId, WalletStorageError> {
        use crate::schema::{proofs, vaults};

        let (vault_id, account_id) = vaults::table
            .select((vaults::id, vaults::account_id))
            .filter(vaults::address.eq(vault_address.to_string()))
            .first::<(i32, i32)>(self.connection())
            .map_err(|e| WalletStorageError::general("proof_insert", e))?;

        diesel::insert_into(proofs::table)
            .values((proofs::account_id.eq(account_id), proofs::vault_id.eq(vault_id)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("proof_insert", e))?;

        // RETURNING only available from SQLite 3.35 https://www.sqlite.org/lang_returning.html
        // TODO: See if we can upgrade SQLite
        let proof_id = proofs::table
            .select(proofs::id)
            .order_by(proofs::id.desc())
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("proof_insert", e))?;

        Ok(proof_id as ConfidentialProofId)
    }

    fn proofs_delete(&mut self, proof_id: ConfidentialProofId) -> Result<(), WalletStorageError> {
        use crate::schema::proofs;

        diesel::delete(proofs::table.filter(proofs::id.eq(proof_id as i32)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("proof_delete", e))?;

        Ok(())
    }

    fn proofs_set_transaction_hash(
        &mut self,
        proof_id: ConfidentialProofId,
        transaction_hash: FixedHash,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::proofs;

        diesel::update(proofs::table.filter(proofs::id.eq(proof_id as i32)))
            .set(proofs::transaction_hash.eq(transaction_hash.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("proofs_set_transaction_hash", e))?;

        Ok(())
    }
}

impl Drop for WriteTransaction<'_> {
    fn drop(&mut self) {
        if !self.transaction.is_done() {
            warn!(target: LOG_TARGET, "WriteTransaction was not committed or rolled back");
            if let Err(err) = self.transaction.rollback() {
                warn!(target: LOG_TARGET, "Failed to rollback WriteTransaction: {}", err);
            }
        }
    }
}

impl<'a> Deref for WriteTransaction<'a> {
    type Target = ReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl<'a> DerefMut for WriteTransaction<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}
