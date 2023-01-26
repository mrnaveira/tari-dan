//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_template_lib::prelude::*;
use tari_template_lib::Hash;

#[derive(Debug, Clone, Encode, Decode, Hash)]
pub enum Emoji {
    Smile,
    Sweat,
    Laugh,
    Wink,
}

#[derive(Debug, Clone, Encode, Decode, Hash)]
pub struct EmojiId {
    pub emojis: Vec<Emoji>,
}

/// This template implements a classic, first come first served, constant price NFT drop
#[template]
mod emoji_id {
    use super::*;  
    
    pub struct EmojiIdMinter {
        max_emoji_id_len: u64,
        mint_price: Amount,
        resource_address: ResourceAddress,
        earnings: Vault,
    }

    impl EmojiIdMinter {
        // TODO: in this example we need to specify the payment resource, but there should be native support for Thaums
        // TODO: decoding fails if "max_emoji_id_len" is usize instead of u64, we may need to add support to it
        pub fn new(payment_resource_address: ResourceAddress, max_emoji_id_len: u64, mint_price: Amount) -> Self {
            // Create the non-fungible resource with empty initial supply
            let resource_address = ResourceBuilder::non_fungible()
                .build(); 
            let earnings = Vault::new_empty(payment_resource_address);
            Self {
                max_emoji_id_len,
                mint_price,
                resource_address,
                earnings,
            }
        }

        // TODO: return change
        pub fn mint(&mut self, emojis: Vec<Emoji>, payment: Bucket) -> Bucket {
            assert!(
                !emojis.is_empty() && emojis.len() as u64 <= self.max_emoji_id_len,
                "Invalid Emoji ID length"
            );

            // process the payment
            // no need to manually check the amount, as the split operation will fail if not enough funds
            // TODO: let (cost, change) = payment.split(self.mint_price);
            // no need to manually check that the payment is in the same resource that we are accepting ...
            // ... the deposit will fail if it's different
            self.earnings.deposit(payment);

            // mint a new emoji id
            // TODO: how do we ensure uniqueness of emoji ids? Two options:
            //      1. Derive the nft id from the emojis
            //      2. Enforce that always an NFT's immutable data must be unique in the resource's scope
            //      3. Ad-hoc uniqueness fields in a NFT resource
            // We are going with (1) for now
            //let hash = Hash::try_from_vec(encode(&emojis).unwrap()).unwrap();
            //let id = NonFungibleId(hash);
            let id = NonFungibleId::random();
            let mut immutable_data = Metadata::new();
            immutable_data.insert("emojis", format!("Emojis: {:?}", emojis));
            let nft = NonFungible::new(immutable_data, &{});
            
            // if a previous emoji id was minted with the same emojis, the hash will be the same
            // so consensus will fail when running "mint_non_fungible"
            let emoji_id_bucket = ResourceManager::get(self.resource_address)
                .mint_non_fungible(id, nft);

            emoji_id_bucket
        }

        pub fn total_supply(&self) -> Amount {
            ResourceManager::get(self.resource_address).total_supply()
        }
    }
}
