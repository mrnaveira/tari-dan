//  Copyright 2023, The Tari Project
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

use std::{str::FromStr, sync::Arc};

use anyhow::anyhow;
use log::info;
use tari_engine_types::substate::{Substate, SubstateAddress};

use crate::{
    dan_layer_scanner::{DanLayerScanner, NonFungible},
    substate_storage_sqlite::{
        models::{
            non_fungible_index::{IndexedNftSubstate, NewNonFungibleIndex},
            substate::{NewSubstate, Substate as SubstateRow},
        },
        sqlite_substate_store_factory::{
            SqliteSubstateStore,
            SqliteSubstateStoreWriteTransaction,
            SubstateStore,
            SubstateStoreReadTransaction,
            SubstateStoreWriteTransaction,
        },
    },
};

const LOG_TARGET: &str = "tari::indexer::substate_manager";

pub struct SubstateManager {
    dan_layer_scanner: Arc<DanLayerScanner>,
    substate_store: SqliteSubstateStore,
}

impl SubstateManager {
    pub fn new(dan_layer_scanner: Arc<DanLayerScanner>, substate_store: SqliteSubstateStore) -> Self {
        Self {
            dan_layer_scanner,
            substate_store,
        }
    }

    pub async fn fetch_and_add_substate_to_db(&self, substate_address: &SubstateAddress) -> Result<(), anyhow::Error> {
        // get the last version of the substate from the dan layer
        // TODO: fetch the last version from database to avoid scaning always from the beginning
        let substate = match self.get_substate_from_dan_layer(substate_address, None).await {
            Some(substate) => substate,
            None => return Err(anyhow!("Substate not found in the network")),
        };

        // if it's a resource, we need also to retrieve all the individual nfts
        let non_fungibles = if let SubstateAddress::Resource(addr) = substate_address {
            // TODO: fetch the last index from database to avoid scaning always from the beginning
            self.dan_layer_scanner.get_non_fungibles(addr, 0, None).await?
        } else {
            vec![]
        };

        // store the substate in the database
        let mut tx = self.substate_store.create_write_tx()?;
        store_substate_in_db(&mut tx, substate_address, &substate)?;
        info!(
            target: LOG_TARGET,
            "Added substate {} with version {} to the database",
            substate_address.to_address_string(),
            substate.version()
        );

        // store the associated non fungibles in the database
        for nft in non_fungibles {
            // store the substate of the nft in the databas
            store_substate_in_db(&mut tx, &nft.address, &nft.substate)?;

            // store the index of the nft
            let nft_index_db_row = map_nft_index_to_db_row(substate_address, &nft)?;
            tx.add_non_fungible_index(nft_index_db_row)?;
            info!(
                target: LOG_TARGET,
                "Added non fungible {} at index {} to the database", nft.address, nft.index,
            );
        }
        tx.commit()?;

        Ok(())
    }

    pub async fn delete_substate_from_db(&self, substate_address: &SubstateAddress) -> Result<(), anyhow::Error> {
        let mut tx = self.substate_store.create_write_tx()?;
        tx.delete_substate(substate_address.to_address_string())?;
        tx.commit()?;

        Ok(())
    }

    pub async fn delete_all_substates_from_db(&self) -> Result<(), anyhow::Error> {
        let mut tx = self.substate_store.create_write_tx()?;
        tx.clear_substates()?;
        tx.commit()?;

        Ok(())
    }

    pub async fn get_all_addresses_from_db(&self) -> Result<Vec<(String, i64)>, anyhow::Error> {
        let mut tx = self.substate_store.create_read_tx()?;
        let addresses = tx.get_all_addresses()?;

        Ok(addresses)
    }

    pub async fn get_substate(
        &self,
        substate_address: &SubstateAddress,
        version: Option<u32>,
    ) -> Result<Option<Substate>, anyhow::Error> {
        // we store the latest version of the substates in the watchlist,
        // so we will return the substate directly from database if it's there
        if let Some(substate) = self.get_substate_from_db(substate_address, version).await? {
            return Ok(Some(substate));
        }

        // the substate is not in db (or is not the requested version) so we fetch it from the dan layer commitee
        let substate = self.get_substate_from_dan_layer(substate_address, version).await;
        Ok(substate)
    }

    async fn get_substate_from_db(
        &self,
        substate_address: &SubstateAddress,
        version: Option<u32>,
    ) -> Result<Option<Substate>, anyhow::Error> {
        let address_str = substate_address.to_address_string();

        let mut tx = self.substate_store.create_read_tx()?;
        if let Some(row) = tx.get_substate(address_str)? {
            // if a version is requested, we must check that it matches the one in db
            if let Some(version) = version {
                if i64::from(version) != row.version {
                    return Ok(None);
                }
            }

            // the substate is present in db and the version matches the requested version
            let substate = map_db_row_to_substate(&row)?;
            return Ok(Some(substate));
        };

        // the substate is not present in db
        Ok(None)
    }

    async fn get_substate_from_dan_layer(
        &self,
        substate_address: &SubstateAddress,
        version: Option<u32>,
    ) -> Option<Substate> {
        self.dan_layer_scanner.get_substate(substate_address, version).await
    }

    pub async fn get_non_fungible_count(&self, substate_address: &SubstateAddress) -> Result<i64, anyhow::Error> {
        let address_str = substate_address.to_address_string();
        let mut tx = self.substate_store.create_read_tx()?;
        tx.get_non_fungible_count(address_str).map_err(|e| e.into())
    }

    pub async fn get_non_fungibles(
        &self,
        substate_address: &SubstateAddress,
        start_index: u64,
        end_index: u64,
    ) -> Result<Vec<NonFungible>, anyhow::Error> {
        let address_str = substate_address.to_address_string();
        let mut tx = self.substate_store.create_read_tx()?;
        let db_rows = tx.get_non_fungibles(address_str, start_index as i32, end_index as i32)?;

        let mut nfts = vec![];
        for row in db_rows {
            nfts.push(map_db_row_to_nft_index(&row)?);
        }
        Ok(nfts)
    }

    pub async fn scan_and_update_substates(&self) -> Result<(), anyhow::Error> {
        let addresses = self.get_all_addresses_from_db().await?;

        for (address, _) in addresses {
            let address = SubstateAddress::from_str(&address)?;
            self.fetch_and_add_substate_to_db(&address).await?;
        }

        Ok(())
    }
}

fn store_substate_in_db(
    tx: &mut SqliteSubstateStoreWriteTransaction,
    address: &SubstateAddress,
    substate: &Substate,
) -> Result<(), anyhow::Error> {
    let substate_row = map_substate_to_db_row(address, substate)?;
    tx.set_substate(substate_row)?;

    Ok(())
}

fn map_db_row_to_substate(row: &SubstateRow) -> Result<Substate, anyhow::Error> {
    let substate: Substate = serde_json::from_str(&row.data)?;
    Ok(substate)
}

fn map_substate_to_db_row(address: &SubstateAddress, substate: &Substate) -> Result<NewSubstate, anyhow::Error> {
    let pretty_data = serde_json::to_string_pretty(&substate)?;
    let row = NewSubstate {
        address: address.to_address_string(),
        version: i64::from(substate.version()),
        data: pretty_data,
    };
    Ok(row)
}

fn map_db_row_to_nft_index(row: &IndexedNftSubstate) -> Result<NonFungible, anyhow::Error> {
    let substate: Substate = serde_json::from_str(&row.data)?;
    let address = SubstateAddress::from_str(&row.address)?;
    Ok(NonFungible {
        index: row.idx as u64,
        address,
        substate,
    })
}

fn map_nft_index_to_db_row(
    resource_address: &SubstateAddress,
    nft: &NonFungible,
) -> Result<NewNonFungibleIndex, anyhow::Error> {
    Ok(NewNonFungibleIndex {
        resource_address: resource_address.to_address_string(),
        idx: nft.index as i32,
        non_fungible_address: nft.address.to_address_string(),
    })
}
