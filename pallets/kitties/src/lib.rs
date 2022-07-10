#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_support::{
		codec::{Decode, Encode, MaxEncodedLen},
		scale_info::TypeInfo,
		sp_runtime::traits::Hash,
		traits::{tokens::ExistenceRequirement, Currency, Randomness},
		transactional,
	};
	use frame_system::{pallet_prelude::*, Origin};
	use sp_io::hashing::blake2_128;

	type AccountOf<T> = <T as frame_system::Config>::AccountId;
	pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Clone, PartialEq, Eq, RuntimeDebug, Encode, Decode, TypeInfo, MaxEncodedLen)]
	#[scale_info(skip_type_params(T))]
	#[codec(mel_bound())]
	pub struct Kitty<T: Config> {
		pub dna: [u8; 16],
		pub price: Option<BalanceOf<T>>,
		pub gender: Gender,
		pub owner: AccountOf<T>,
	}

	#[derive(Clone, PartialEq, Eq, RuntimeDebug, Encode, Decode, TypeInfo, MaxEncodedLen)]
	// #[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
	pub enum Gender {
		#[codec(index = 0)]
		Male,
		#[codec(index = 1)]
		Female,
	}

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// The Currency handler for the Kitties pallet.
		type Currency: Currency<Self::AccountId>;

		/// The maximum amount of Kitties a single account can own.
		#[pallet::constant]
		type MaxKittyOwned: Get<u32>;

		/// The type of Randomness we want to specify for this pallet.
		type KittyRandomness: Randomness<Self::Hash, Self::BlockNumber>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn kitty_count)]
	// Learn more about declaring storage items:
	// https://docs.substrate.io/v3/runtime/storage#declaring-storage-items
	pub(super) type KittyCount<T> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn kitties)]
	pub(super) type Kitties<T: Config> =
		StorageMap<_, Twox64Concat, T::Hash, Kitty<T>>;

	#[pallet::storage]
	#[pallet::getter(fn kitties_owned)]
	pub(super) type KittiesOwned<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		BoundedVec<T::Hash, T::MaxKittyOwned>,
		ValueQuery,
	>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored(u32, T::AccountId),
		Created(T::AccountId, T::Hash),
		PriceSet(T::AccountId, T::Hash, Option<BalanceOf<T>>),
		Transferred(T::AccountId, T::AccountId, T::Hash),
		Bought(T::AccountId, T::AccountId, T::Hash, Option<BalanceOf<T>>),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
		KittyCountOverflow,
		ExceedMaxKittyOwned,
		KittyNoExist,
		NotKittyOwner,
		TransferToYourself,
		KittyIdNoExist,
		BuyerIsKittyOwner,
		KittyNotReadyForSale,
		NotEnoughBalance,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(100)]
		pub fn create_kitty(origin: OriginFor<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;

			let kitty_id = Self::mint(&who, None, None)?;

			log::info!("A kitty is born with ID: {:?}.", kitty_id);

			Self::deposit_event(Event::Created(who, kitty_id));

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn set_price(
			origin: OriginFor<T>,
			kitty_id: T::Hash,
			new_price: Option<BalanceOf<T>>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			let mut kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNoExist)?;

			kitty.price = new_price.clone();

			<Kitties<T>>::insert(kitty_id, kitty);

			Self::deposit_event(Event::PriceSet(who, kitty_id, new_price));

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn transfer(
			origin: OriginFor<T>,
			to: T::AccountId,
			kitty_id: T::Hash,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;

			ensure!(Self::is_kitty_owner(&who, &kitty_id)?, <Error<T>>::NotKittyOwner);

			ensure!(who != to, <Error<T>>::TransferToYourself);

			ensure!(!Self::is_exceed_max_kitty(&who), <Error<T>>::ExceedMaxKittyOwned);

			Self::transfer_kitty_to(&to, &kitty_id);

			Self::deposit_event(Event::Transferred(who, to, kitty_id));

			Ok(())
		}

		#[transactional]
		#[pallet::weight(100)]
		pub fn buy_kitty(origin: OriginFor<T>, kitty_id: T::Hash) -> DispatchResult {
			let who = ensure_signed(origin)?;

			// get kitty
			let kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNoExist)?;

			// check this kitty is ready for sale
			if kitty.price.is_none() {
				Err(<Error<T>>::NotEnoughBalance)?;
			}
			let kitty_price = kitty.price.unwrap();

			// check balance
			ensure!(T::Currency::free_balance(&who) > kitty_price, <Error<T>>::NotEnoughBalance);

			// check is from equal kitty owner
			ensure!(who != kitty.owner, <Error<T>>::BuyerIsKittyOwner);
			let seller = kitty.owner.clone();

			// transfer currency
			T::Currency::transfer(&who, &seller, kitty_price, ExistenceRequirement::KeepAlive)?;

			// transfer kitty to buyer
			Self::transfer_kitty_to(&who, &kitty_id);

			// deposit event
			Self::deposit_event(Event::Bought(who, seller, kitty_id, kitty.price));

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn get_gender() -> Gender {
			let random = T::KittyRandomness::random(&b"gender"[..]).0;

			match random.as_ref()[0] % 2 {
				0 => Gender::Male,
				_ => Gender::Female,
			}
		}

		fn gen_dna() -> [u8; 16] {
			let payload = (
				T::KittyRandomness::random(&b"dna"[..]).0,
				<frame_system::Pallet<T>>::block_number(),
			);
			payload.using_encoded(blake2_128)
		}

		pub fn mint(
			owner: &T::AccountId,
			dna: Option<[u8; 16]>,
			gender: Option<Gender>,
		) -> Result<T::Hash, Error<T>> {
			// let kitty = Kitty::<BalanceOf<T>, T::AccountId> {
			let kitty = Kitty::<T> {
				dna: dna.unwrap_or_else(Self::gen_dna),
				gender: gender.unwrap_or_else(Self::get_gender),
				price: None,
				owner: owner.clone(),
			};

			let kitty_id = T::Hashing::hash_of(&kitty);

			let new_count =
				Self::kitty_count().checked_add(1).ok_or(Error::<T>::KittyCountOverflow)?;
			<KittiesOwned<T>>::try_mutate(&owner, |kitty_vec| kitty_vec.try_push(kitty_id))
				.map_err(|_| <Error<T>>::ExceedMaxKittyOwned)?;

			<Kitties<T>>::insert(kitty_id, kitty);
			<KittyCount<T>>::put(new_count);

			Ok(kitty_id)
		}

		pub fn is_kitty_owner(owner: &T::AccountId, kitty_id: &T::Hash) -> Result<bool, Error<T>> {
			match Self::kitties(kitty_id) {
				Some(kitty) => Ok(kitty.owner == *owner),
				None => Err(<Error<T>>::KittyNoExist),
			}
		}

		pub fn is_exceed_max_kitty(owner: &T::AccountId) -> bool {
			let kitties = Self::kitties_owned(owner);

			(kitties.len() as u32) >= T::MaxKittyOwned::get()
		}

		pub fn transfer_kitty_to(to: &T::AccountId, kitty_id: &T::Hash) -> Result<(), Error<T>> {
			let mut kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNoExist)?;

			let prev_owner = kitty.owner.clone();

			<KittiesOwned<T>>::try_mutate(&prev_owner, |kitty_ids| {
				if let Some(pos) = kitty_ids.iter().position(|x| *x == *kitty_id) {
					kitty_ids.swap_remove(pos);
					return Ok(());
				}

				Err(())
			})
			.map_err(|_| <Error<T>>::KittyNoExist)?;

			kitty.owner = to.clone();

			kitty.price = None;

			<Kitties<T>>::insert(kitty_id, kitty);

			<KittiesOwned<T>>::try_mutate(&to, |kitty_ids| kitty_ids.try_push(*kitty_id))
				.map_err(|_| <Error<T>>::ExceedMaxKittyOwned)?;

			Ok(())
		}
	}
}
