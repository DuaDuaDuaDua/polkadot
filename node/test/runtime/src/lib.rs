// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! End to end runtime tests

use test_runner::{Node, ChainInfo, SignatureVerificationOverride, default_config};
use grandpa::GrandpaBlockImport;
use sc_service::{TFullBackend, TFullClient, Configuration, TaskManager, new_full_parts, TaskExecutor};
use sp_runtime::generic::Era;
use std::sync::Arc;
use sc_consensus_babe::BabeBlockImport;
use sp_keystore::SyncCryptoStorePtr;
use sp_keyring::sr25519::Keyring::Alice;
use polkadot_runtime_common::claims;
use sp_consensus_babe::AuthorityId;
use sc_consensus_manual_seal::{ConsensusDataProvider, consensus::babe::BabeConsensusDataProvider};
use sp_runtime::AccountId32;
use frame_support::{weights::Weight, StorageValue};
use pallet_democracy::{AccountVote, Conviction, Vote};
use polkadot_runtime::{FastTrackVotingPeriod, Runtime, RuntimeApi, Event, TechnicalCollective, CouncilCollective};
use polkadot_service::chain_spec::polkadot_development_config;
use std::str::FromStr;
use codec::Encode;
use sc_consensus_manual_seal::consensus::babe::{SlotTimestampProvider, BabeVerifier};
use sp_consensus::import_queue::BasicQueue;
use sp_inherents::CreateInherentDataProviders;
use sp_runtime::app_crypto::sp_core::H256;
use sp_api::TransactionFor;

pub type BlockImport<B, BE, C, SC> = BabeBlockImport<B, C, GrandpaBlockImport<BE, B, C, SC>>;
pub type Block = polkadot_primitives::v1::Block;
pub type SelectChain = sc_consensus::LongestChain<TFullBackend<Block>, Block>;

sc_executor::native_executor_instance!(
	pub Executor,
	polkadot_runtime::api::dispatch,
	polkadot_runtime::native_version,
	(frame_benchmarking::benchmarking::HostFunctions, SignatureVerificationOverride),
);

/// ChainInfo implementation.
pub struct PolkadotChainInfo;

impl ChainInfo for PolkadotChainInfo {
    type Block = Block;
    type Executor = Executor;
    type Runtime = Runtime;
    type RuntimeApi = RuntimeApi;
    type SelectChain = SelectChain;
    type BlockImport = BlockImport<
        Self::Block,
        TFullBackend<Self::Block>,
        TFullClient<Self::Block, RuntimeApi, Self::Executor>,
        Self::SelectChain,
    >;
    type SignedExtras = polkadot_runtime::SignedExtra;
    type InherentDataProviders = (SlotTimestampProvider, sp_consensus_babe::inherents::InherentDataProvider);

    fn signed_extras(from: <Runtime as frame_system::Config>::AccountId) -> Self::SignedExtras {
        (
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            frame_system::CheckMortality::<Runtime>::from(Era::Immortal),
            frame_system::CheckNonce::<Runtime>::from(frame_system::Pallet::<Runtime>::account_nonce(from)),
            frame_system::CheckWeight::<Runtime>::new(),
            pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(0),
            claims::PrevalidateAttests::<Runtime>::new(),
        )
    }
}

/// Dispatch with pallet_democracy
pub async fn dispatch_with_pallet_democracy<T>(call: <T::Runtime as frame_system::Config>::Call, node: &Node<T>)
    -> Result<(), sc_transaction_pool::error::Error>
    where
        T: ChainInfo<
            Block = Block,
            Executor = Executor,
            Runtime = Runtime,
            RuntimeApi = RuntimeApi,
            SelectChain = SelectChain,
            BlockImport = BlockImport<
                Block,
                TFullBackend<Block>,
                TFullClient<Block, RuntimeApi, Executor>,
                SelectChain,
            >,
            SignedExtras = polkadot_runtime::SignedExtra
        >
{
    type DemocracyCall = pallet_democracy::Call<Runtime>;
    type TechnicalCollectiveCall = pallet_collective::Call<Runtime, TechnicalCollective>;
    type CouncilCollectiveCall = pallet_collective::Call<Runtime, CouncilCollective>;
    type CouncilEvent = pallet_collective::Event::<Runtime, CouncilCollective>;
    type TechnicalCommitteeEvent = pallet_collective::Event::<Runtime, TechnicalCollective>;

    // here lies a black mirror esque copy of on chain whales.
    let whales = vec![
        "1rvXMZpAj9nKLQkPFCymyH7Fg3ZyKJhJbrc7UtHbTVhJm1A",
        "15j4dg5GzsL1bw2U2AWgeyAk6QTxq43V7ZPbXdAmbVLjvDCK",
    ]
        .into_iter()
        .map(|account| AccountId32::from_str(account).unwrap())
        .collect::<Vec<_>>();

    // and these
    let (technical_collective, council_collective) = node.with_state(|| (
        pallet_collective::Members::<Runtime, TechnicalCollective>::get(),
        pallet_collective::Members::<Runtime, CouncilCollective>::get()
    ));

    // note the call (pre-image?) of the call.
    node.submit_extrinsic(DemocracyCall::note_preimage(call.encode()), whales[0].clone()).await?;
    node.seal_blocks(1).await;

    // fetch proposal hash from event emitted by the runtime
    let events = node.events();
    let proposal_hash = events.into_iter()
        .filter_map(|event| match event.event {
            Event::Democracy(
                pallet_democracy::Event::PreimageNoted(proposal_hash, _, _)
            ) => Some(proposal_hash),
            _ => None
        })
        .next()
        .unwrap();

    // submit external_propose call through council
    let external_propose = DemocracyCall::external_propose_majority(proposal_hash.clone().into());
    let proposal_length = external_propose.using_encoded(|x| x.len()) as u32 + 1;
    let proposal_weight = Weight::MAX / 100_000_000;
    let proposal = CouncilCollectiveCall::propose(
        council_collective.len() as u32,
        Box::new(external_propose.clone().into()),
        proposal_length
    );

    node.submit_extrinsic(proposal.clone(), council_collective[0].clone()).await?;
    node.seal_blocks(1).await;

    // fetch proposal index from event emitted by the runtime
    let events = node.events();
    let (council_proposal_index, council_proposal_hash): (u32, H256) = events.into_iter()
        .filter_map(|event| {
            match event.event {
                Event::Council(CouncilEvent::Proposed(_, index, proposal_hash, _)) => Some((index, proposal_hash)),
                _ => None
            }
        })
        .next()
        .unwrap();

    // vote
    for member in &council_collective[1..] {
        let call = CouncilCollectiveCall::vote(council_proposal_hash.clone(), council_proposal_index, true);
        node.submit_extrinsic(call, member.clone()).await?;
    }
    node.seal_blocks(1).await;

    // close vote
    let call = CouncilCollectiveCall::close(council_proposal_hash, council_proposal_index, proposal_weight, proposal_length);
    node.submit_extrinsic(call, council_collective[0].clone()).await?;
    node.seal_blocks(1).await;

    // assert that proposal has been passed on chain
    let events = node.events()
        .into_iter()
        .filter(|event| {
            match event.event {
                Event::Council(CouncilEvent::Closed(_, _, _)) |
                Event::Council(CouncilEvent::Approved(_,)) |
                Event::Council(CouncilEvent::Executed(_, Ok(()))) => true,
                _ => false,
            }
        })
        .collect::<Vec<_>>();

    // make sure all 3 events are in state
    assert_eq!(events.len(), 3);

    // next technical collective must fast track the proposal.
    let fast_track = DemocracyCall::fast_track(proposal_hash.into(), FastTrackVotingPeriod::get(), 0);
    let proposal_weight = Weight::MAX / 100_000_000;
    let fast_track_length = fast_track.using_encoded(|x| x.len()) as u32 + 1;
    let proposal = TechnicalCollectiveCall::propose(
        technical_collective.len() as u32,
        Box::new(fast_track.into()),
        fast_track_length
    );

    node.submit_extrinsic(proposal, technical_collective[0].clone()).await?;
    node.seal_blocks(1).await;

    let (technical_proposal_index, technical_proposal_hash) = node.events()
        .into_iter()
        .filter_map(|event| {
            match event.event {
                Event::TechnicalCommittee(
                    TechnicalCommitteeEvent::Proposed(_, index, hash, _)
                ) => Some((index, hash)),
                _ => None
            }
        })
        .next()
        .unwrap();

    // vote
    for member in &technical_collective[1..] {
        let call = TechnicalCollectiveCall::vote(technical_proposal_hash.clone(), technical_proposal_index, true);
        node.submit_extrinsic(call, member.clone()).await?;
    }
    node.seal_blocks(1).await;

    // close vote
    let call = TechnicalCollectiveCall::close(
        technical_proposal_hash,
        technical_proposal_index,
        proposal_weight,
        fast_track_length,
    );
    node.submit_extrinsic(call, technical_collective[0].clone()).await?;
    node.seal_blocks(1).await;

    // assert that fast-track proposal has been passed on chain
    let collective_events = node.events()
        .into_iter()
        .filter(|event| {
            match event.event {
                Event::TechnicalCommittee(TechnicalCommitteeEvent::Closed(_, _, _)) |
                Event::TechnicalCommittee(TechnicalCommitteeEvent::Approved(_)) |
                Event::TechnicalCommittee(TechnicalCommitteeEvent::Executed(_, Ok(()))) => true,
                _ => false,
            }
        })
        .collect::<Vec<_>>();

    // make sure all 3 events are in state
    assert_eq!(collective_events.len(), 3);

    // now runtime upgrade proposal is a fast-tracked referendum we can vote for.
    let referendum_index = events.into_iter()
        .filter_map(|event| match event.event {
            Event::Democracy(pallet_democracy::Event::<Runtime>::Started(index, _)) => Some(index),
            _ => None,
        })
        .next()
        .unwrap();
    let call = DemocracyCall::vote(
        referendum_index,
        AccountVote::Standard {
            vote: Vote { aye: true, conviction: Conviction::Locked1x },
            // 10 DOTS
            balance: 10_000_000_000_000
        }
    );
    for whale in whales {
        node.submit_extrinsic(call.clone(), whale).await?;
    }

    // wait for fast track period.
    node.seal_blocks(FastTrackVotingPeriod::get() as usize).await;

    // assert that the proposal is passed by looking at events
    let events = node.events()
        .into_iter()
        .filter(|event| {
            match event.event {
                Event::Democracy(pallet_democracy::Event::Passed(_)) |
                Event::Democracy(pallet_democracy::Event::PreimageUsed(_, _, _)) |
                Event::Democracy(pallet_democracy::Event::Executed(_, true)) => true,
                _ => false,
            }
        })
        .collect::<Vec<_>>();

    // make sure all events were emitted
    assert_eq!(events.len(), 3);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_runtime::{MultiSigner, traits::IdentifyAccount};
    use test_runner::{ConfigOrChainSpec, client_parts, task_executor, build_runtime};

    #[test]
    fn test_runner() {
        let mut runtime = build_runtime().unwrap();
        let task_executor = task_executor(runtime.handle().clone());
        let (rpc,task_manager, client, pool, command_sink, backend) =
            client_parts::<PolkadotChainInfo>(
                ConfigOrChainSpec::ChainSpec(Box::new(polkadot_development_config()), task_executor)
            )?;
        let node = Node::<PolkadotChainInfo>::new(rpc, task_manager, client, pool, command_sink, backend);

        runtime.block_on(async {
           // seals blocks
           node.seal_blocks(1).await;
           // submit extrinsics
           let alice = MultiSigner::from(Alice.public()).into_account();
           node.submit_extrinsic(frame_system::Call::remark((b"hello world").to_vec()), alice)
               .await
               .unwrap();

           // look ma, I can read state.
           let _events = node.with_state(|| frame_system::Pallet::<Runtime>::events());
           // get access to the underlying client.
           let _client = node.client();
       });
    }
}
