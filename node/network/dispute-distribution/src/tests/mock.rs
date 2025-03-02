// Copyright 2021 Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.
//

//! Mock data and utility functions for unit tests in this subsystem.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use lazy_static::lazy_static;

use polkadot_node_network_protocol::{PeerId, authority_discovery::AuthorityDiscovery};
use sc_keystore::LocalKeystore;
use sp_application_crypto::AppKey;
use sp_keyring::{Sr25519Keyring};
use sp_keystore::{SyncCryptoStore, SyncCryptoStorePtr};

use polkadot_node_primitives::{DisputeMessage, SignedDisputeStatement};
use polkadot_primitives::v1::{
	CandidateDescriptor, CandidateHash, CandidateReceipt, Hash,
	SessionIndex, SessionInfo, ValidatorId, ValidatorIndex, AuthorityDiscoveryId,
};

pub const MOCK_SESSION_INDEX: SessionIndex = 1;
pub const MOCK_NEXT_SESSION_INDEX: SessionIndex = 2;
pub const MOCK_VALIDATORS: [Sr25519Keyring; 6] = [
	Sr25519Keyring::Ferdie,
	Sr25519Keyring::Alice,
	Sr25519Keyring::Bob,
	Sr25519Keyring::Charlie,
	Sr25519Keyring::Dave,
	Sr25519Keyring::Eve,
];

pub const MOCK_AUTHORITIES_NEXT_SESSION: [Sr25519Keyring;2] = [
	Sr25519Keyring::One,
	Sr25519Keyring::Two,
];

pub const FERDIE_INDEX: ValidatorIndex = ValidatorIndex(0);
pub const ALICE_INDEX: ValidatorIndex = ValidatorIndex(1);


lazy_static! {

/// Mocked AuthorityDiscovery service.
pub static ref MOCK_AUTHORITY_DISCOVERY: MockAuthorityDiscovery = MockAuthorityDiscovery::new();
// Creating an innocent looking `SessionInfo` is really expensive in a debug build. Around
// 700ms on my machine, We therefore cache those keys here:
pub static ref MOCK_VALIDATORS_DISCOVERY_KEYS: HashMap<Sr25519Keyring, AuthorityDiscoveryId> = 
	MOCK_VALIDATORS
	.iter()
	.chain(MOCK_AUTHORITIES_NEXT_SESSION.iter())
	.map(|v| (v.clone(), v.public().into()))
	.collect()
;
pub static ref FERDIE_DISCOVERY_KEY: AuthorityDiscoveryId =
	MOCK_VALIDATORS_DISCOVERY_KEYS.get(&Sr25519Keyring::Ferdie).unwrap().clone();

pub static ref MOCK_SESSION_INFO: SessionInfo =
	SessionInfo {
		validators: MOCK_VALIDATORS.iter().take(4).map(|k| k.public().into()).collect(),
		discovery_keys: MOCK_VALIDATORS
			.iter()
			.map(|k| MOCK_VALIDATORS_DISCOVERY_KEYS.get(&k).unwrap().clone())
			.collect(),
		..Default::default()
	};

/// SessionInfo for the second session. (No more validators, but two more authorities.
pub static ref MOCK_NEXT_SESSION_INFO: SessionInfo =
	SessionInfo {
		discovery_keys:
			MOCK_AUTHORITIES_NEXT_SESSION
				.iter()
				.map(|k| MOCK_VALIDATORS_DISCOVERY_KEYS.get(&k).unwrap().clone())
				.collect(),
		..Default::default()
	};
}


pub fn make_candidate_receipt(relay_parent: Hash) -> CandidateReceipt {
	CandidateReceipt {
		descriptor: CandidateDescriptor {
			relay_parent,
			..Default::default()
		},
		commitments_hash: Hash::random(),
	}
}

pub async fn make_explicit_signed(
	validator: Sr25519Keyring,
	candidate_hash: CandidateHash,
	valid: bool
) -> SignedDisputeStatement {
	let keystore: SyncCryptoStorePtr = Arc::new(LocalKeystore::in_memory());
	SyncCryptoStore::sr25519_generate_new(
		&*keystore,
		ValidatorId::ID,
		Some(&validator.to_seed()),
	)
	.expect("Insert key into keystore");

	SignedDisputeStatement::sign_explicit(
		&keystore,
		valid,
		candidate_hash,
		MOCK_SESSION_INDEX,
		validator.public().into(),
	)
	.await
	.expect("Keystore should be fine.")
	.expect("Signing should work.")
}


pub async fn make_dispute_message(
	candidate: CandidateReceipt,
	valid_validator: ValidatorIndex,
	invalid_validator: ValidatorIndex,
) -> DisputeMessage {
	let candidate_hash = candidate.hash();
	let valid_vote = 
		make_explicit_signed(MOCK_VALIDATORS[valid_validator.0 as usize], candidate_hash, true).await;
	let invalid_vote =
		make_explicit_signed(MOCK_VALIDATORS[invalid_validator.0 as usize], candidate_hash, false).await;
	DisputeMessage::from_signed_statements(
		valid_vote,
		valid_validator,
		invalid_vote,
		invalid_validator,
		candidate,
		&MOCK_SESSION_INFO,
	)
	.expect("DisputeMessage construction should work.")
}

/// Dummy `AuthorityDiscovery` service.
#[derive(Debug, Clone)]
pub struct MockAuthorityDiscovery {
	peer_ids: HashMap<Sr25519Keyring, PeerId>
}

impl MockAuthorityDiscovery {
	pub fn new() -> Self {
		let mut peer_ids = HashMap::new();
		peer_ids.insert(Sr25519Keyring::Alice, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Bob, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Ferdie, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Charlie, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Dave, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Eve, PeerId::random());
		peer_ids.insert(Sr25519Keyring::One, PeerId::random());
		peer_ids.insert(Sr25519Keyring::Two, PeerId::random());

		Self { peer_ids }
	}

	pub fn get_peer_id_by_authority(&self, authority: Sr25519Keyring) -> PeerId {
		*self.peer_ids.get(&authority).expect("Tester only picks valid authorities")
	}
}

#[async_trait]
impl AuthorityDiscovery for MockAuthorityDiscovery {
	async fn get_addresses_by_authority_id(&mut self, _authority: polkadot_primitives::v1::AuthorityDiscoveryId)
		-> Option<Vec<sc_network::Multiaddr>> {
			panic!("Not implemented");
	}

	async fn get_authority_id_by_peer_id(&mut self, peer_id: polkadot_node_network_protocol::PeerId)
		-> Option<polkadot_primitives::v1::AuthorityDiscoveryId> {
		for (a, p) in self.peer_ids.iter() {
			if p == &peer_id {
				return Some(MOCK_VALIDATORS_DISCOVERY_KEYS.get(&a).unwrap().clone())
			}
		}
		None
	}
}
