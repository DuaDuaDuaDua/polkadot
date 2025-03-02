// Copyright 2019-2021 Parity Technologies (UK) Ltd.
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

//! Common runtime code for Polkadot and Kusama.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod claims;
pub mod slots;
pub mod auctions;
pub mod crowdloan;
pub mod purchase;
pub mod impls;
pub mod mmr;
pub mod paras_sudo_wrapper;
pub mod paras_registrar;
pub mod slot_range;
pub mod traits;
pub mod xcm_sender;
pub mod elections;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod integration_tests;

use beefy_primitives::crypto::AuthorityId as BeefyId;
use primitives::v1::{AccountId, AssignmentId, BlockNumber, ValidatorId};
use sp_runtime::{Perquintill, Perbill, FixedPointNumber};
use frame_system::limits;
use frame_support::{
	parameter_types, traits::{Currency, OneSessionHandler},
	weights::{Weight, constants::WEIGHT_PER_SECOND, DispatchClass},
};
use pallet_transaction_payment::{TargetedFeeAdjustment, Multiplier};
use static_assertions::const_assert;
pub use frame_support::weights::constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight};

#[cfg(feature = "std")]
pub use pallet_staking::StakerStatus;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use pallet_timestamp::Call as TimestampCall;
pub use pallet_balances::Call as BalancesCall;
pub use elections::{OffchainSolutionLengthLimit, OffchainSolutionWeightLimit};

/// Implementations of some helper traits passed into runtime modules as associated types.
pub use impls::ToAuthor;

pub type NegativeImbalance<T> = <pallet_balances::Pallet<T> as Currency<<T as frame_system::Config>::AccountId>>::NegativeImbalance;

/// We assume that an on-initialize consumes 1% of the weight on average, hence a single extrinsic
/// will not be allowed to consume more than `AvailableBlockRatio - 1%`.
pub const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(1);
/// We allow `Normal` extrinsics to fill up the block up to 75%, the rest can be used
/// by  Operational  extrinsics.
const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
/// We allow for 2 seconds of compute with a 6 second average block time.
pub const MAXIMUM_BLOCK_WEIGHT: Weight = 2 * WEIGHT_PER_SECOND;

const_assert!(NORMAL_DISPATCH_RATIO.deconstruct() >= AVERAGE_ON_INITIALIZE_RATIO.deconstruct());

// Common constants used in all runtimes.
parameter_types! {
	pub const BlockHashCount: BlockNumber = 2400;
	/// The portion of the `NORMAL_DISPATCH_RATIO` that we adjust the fees with. Blocks filled less
	/// than this will decrease the weight and more will increase.
	pub const TargetBlockFullness: Perquintill = Perquintill::from_percent(25);
	/// The adjustment variable of the runtime. Higher values will cause `TargetBlockFullness` to
	/// change the fees more rapidly.
	pub AdjustmentVariable: Multiplier = Multiplier::saturating_from_rational(3, 100_000);
	/// Minimum amount of the multiplier. This value cannot be too low. A test case should ensure
	/// that combined with `AdjustmentVariable`, we can recover from the minimum.
	/// See `multiplier_can_grow_from_zero`.
	pub MinimumMultiplier: Multiplier = Multiplier::saturating_from_rational(1, 1_000_000u128);
	/// Maximum length of block. Up to 5MB.
	pub BlockLength: limits::BlockLength =
		limits::BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
	/// Block weights base values and limits.
	pub BlockWeights: limits::BlockWeights = limits::BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have an extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT,
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();
}

/// Parameterized slow adjusting fee updated based on
/// https://w3f-research.readthedocs.io/en/latest/polkadot/Token%20Economics.html#-2.-slow-adjusting-mechanism
pub type SlowAdjustingFeeUpdate<R> = TargetedFeeAdjustment<
	R,
	TargetBlockFullness,
	AdjustmentVariable,
	MinimumMultiplier
>;

/// The type used for currency conversion.
///
/// This must only be used as long as the balance type is u128.
pub type CurrencyToVote = frame_support::traits::U128CurrencyToVote;
static_assertions::assert_eq_size!(primitives::v1::Balance, u128);

/// A placeholder since there is currently no provided session key handler for parachain validator
/// keys.
pub struct ParachainSessionKeyPlaceholder<T>(sp_std::marker::PhantomData<T>);
impl<T> sp_runtime::BoundToRuntimeAppPublic for ParachainSessionKeyPlaceholder<T> {
	type Public = ValidatorId;
}

impl<T: pallet_session::Config> OneSessionHandler<T::AccountId> for ParachainSessionKeyPlaceholder<T>
{
	type Key = ValidatorId;

	fn on_genesis_session<'a, I: 'a>(_validators: I) where
		I: Iterator<Item = (&'a T::AccountId, ValidatorId)>,
		T::AccountId: 'a
	{

	}

	fn on_new_session<'a, I: 'a>(_changed: bool, _v: I, _q: I) where
		I: Iterator<Item = (&'a T::AccountId, ValidatorId)>,
		T::AccountId: 'a
	{

	}

	fn on_disabled(_: usize) { }
}

/// A placeholder since there is currently no provided session key handler for parachain validator
/// keys.
pub struct AssignmentSessionKeyPlaceholder<T>(sp_std::marker::PhantomData<T>);
impl<T> sp_runtime::BoundToRuntimeAppPublic for AssignmentSessionKeyPlaceholder<T> {
	type Public = AssignmentId;
}

impl<T: pallet_session::Config> OneSessionHandler<T::AccountId> for AssignmentSessionKeyPlaceholder<T>
{
	type Key = AssignmentId;

	fn on_genesis_session<'a, I: 'a>(_validators: I) where
		I: Iterator<Item = (&'a T::AccountId, AssignmentId)>,
		T::AccountId: 'a
	{

	}

	fn on_new_session<'a, I: 'a>(_changed: bool, _v: I, _q: I) where
		I: Iterator<Item = (&'a T::AccountId, AssignmentId)>,
		T::AccountId: 'a
	{

	}

	fn on_disabled(_: usize) { }
}

/// Generates a `BeefyId` from the given `AccountId`. The resulting `BeefyId` is
/// a dummy value and this is a utility function meant to be used when migration
/// session keys.
pub fn dummy_beefy_id_from_account_id(a: AccountId) -> BeefyId {
	let mut id = BeefyId::default();
	let id_raw: &mut [u8] = id.as_mut();

	// NOTE: AccountId is 32 bytes, whereas BeefyId is 33 bytes.
	id_raw[1..].copy_from_slice(a.as_ref());
	id_raw[0..4].copy_from_slice(b"beef");

	id
}

#[cfg(test)]
mod multiplier_tests {
	use super::*;
	use frame_support::{parameter_types, weights::Weight};
	use sp_core::H256;
	use sp_runtime::{
		testing::Header,
		traits::{BlakeTwo256, IdentityLookup, Convert, One},
		Perbill,
	};

	type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Runtime>;
	type Block = frame_system::mocking::MockBlock<Runtime>;

	frame_support::construct_runtime!(
		pub enum Runtime where
			Block = Block,
			NodeBlock = Block,
			UncheckedExtrinsic = UncheckedExtrinsic,
		{
			System: frame_system::{Pallet, Call, Config, Storage, Event<T>}
		}
	);

	parameter_types! {
		pub const BlockHashCount: u64 = 250;
		pub const AvailableBlockRatio: Perbill = Perbill::one();
		pub BlockLength: frame_system::limits::BlockLength =
			frame_system::limits::BlockLength::max(2 * 1024);
		pub BlockWeights: frame_system::limits::BlockWeights =
			frame_system::limits::BlockWeights::simple_max(1024);
	}

	impl frame_system::Config for Runtime {
		type BaseCallFilter = frame_support::traits::AllowAll;
		type BlockWeights = BlockWeights;
		type BlockLength = ();
		type DbWeight = ();
		type Origin = Origin;
		type Index = u64;
		type BlockNumber = u64;
		type Call = Call;
		type Hash = H256;
		type Hashing = BlakeTwo256;
		type AccountId = u64;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type Event = Event;
		type BlockHashCount = BlockHashCount;
		type Version = ();
		type PalletInfo = PalletInfo;
		type AccountData = ();
		type OnNewAccount = ();
		type OnKilledAccount = ();
		type SystemWeightInfo = ();
		type SS58Prefix = ();
		type OnSetCode = ();
	}

	fn run_with_system_weight<F>(w: Weight, mut assertions: F) where F: FnMut() -> () {
		let mut t: sp_io::TestExternalities =
			frame_system::GenesisConfig::default().build_storage::<Runtime>().unwrap().into();
		t.execute_with(|| {
			System::set_block_consumed_resources(w, 0);
			assertions()
		});
	}

	#[test]
	fn multiplier_can_grow_from_zero() {
		let minimum_multiplier = MinimumMultiplier::get();
		let target = TargetBlockFullness::get() *
			BlockWeights::get().get(DispatchClass::Normal).max_total.unwrap();
		// if the min is too small, then this will not change, and we are doomed forever.
		// the weight is 1/100th bigger than target.
		run_with_system_weight(target * 101 / 100, || {
			let next = SlowAdjustingFeeUpdate::<Runtime>::convert(minimum_multiplier);
			assert!(next > minimum_multiplier, "{:?} !>= {:?}", next, minimum_multiplier);
		})
	}

	#[test]
	#[ignore]
	fn multiplier_growth_simulator() {
		// assume the multiplier is initially set to its minimum. We update it with values twice the
		//target (target is 25%, thus 50%) and we see at which point it reaches 1.
		let mut multiplier = MinimumMultiplier::get();
		let block_weight = TargetBlockFullness::get()
			* BlockWeights::get().get(DispatchClass::Normal).max_total.unwrap()
			* 2;
		let mut blocks = 0;
		while multiplier <= Multiplier::one() {
			run_with_system_weight(block_weight, || {
				let next = SlowAdjustingFeeUpdate::<Runtime>::convert(multiplier);
				// ensure that it is growing as well.
				assert!(next > multiplier, "{:?} !>= {:?}", next, multiplier);
				multiplier = next;
			});
			blocks += 1;
			println!("block = {} multiplier {:?}", blocks, multiplier);
		}
	}

	#[test]
	fn generate_dummy_unique_beefy_id_from_account_id() {
		let acc1 = AccountId::new([0; 32]);
		let acc2 = AccountId::new([1; 32]);

		let beefy_id1 = dummy_beefy_id_from_account_id(acc1);
		let beefy_id2 = dummy_beefy_id_from_account_id(acc2);

		assert_ne!(beefy_id1, beefy_id2);
	}
}
