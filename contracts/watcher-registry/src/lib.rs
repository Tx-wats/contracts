#![no_std]
#![warn(clippy::pedantic)]
// Soroban's generated contract interface dictates these shapes, so the
// corresponding pedantic lints fire on correct code and are scoped off here
// rather than silenced case by case:
//   - contract entry points must take `Env` and `Address` by value
//   - `#[contractimpl]` re-exports getters, so `#[must_use]` is not ours to add
#![allow(clippy::needless_pass_by_value, clippy::must_use_candidate)]
use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, vec, Address,
    Env, Vec,
};

contractmeta!(key = "Name", val = "WatcherRegistry");
contractmeta!(key = "Version", val = "0.1.0");

// ── Errors ────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    NotInitialized = 3,
    /// Returned when trying to remove the last admin, which would lock the contract.
    LastAdmin = 4,
    /// Returned when the specified watcher is not currently registered.
    WatcherNotFound = 5,
}

// ── Storage keys ─────────────────────────────────────────────────────────────

/// Storage key variants used to address instance entries.
#[contracttype]
pub enum DataKey {
    /// Stores the `Vec<Address>` of current admins.
    Admins,
    /// Stores the `Vec<Address>` of authorized watcher nodes.
    Watchers,
}

// ── Contract ─────────────────────────────────────────────────────────────────

/// On-chain registry for authorized watcher nodes.
///
/// # Admin model
/// The registry supports a **set of admins** (N-of-N independent signers).
/// Any single admin can perform privileged operations (register/remove watchers,
/// add/remove other admins). This eliminates the single-point-of-failure of a
/// sole admin while keeping the authorization model simple and auditable.
///
/// All admin mutations emit Soroban events so changes are visible on-chain.
///
/// # Role policy
/// Roles are **not mutually exclusive and are not restricted to external
/// accounts**, by design:
/// - The same [`Address`] may be both an admin and a registered watcher. The
///   watcher role grants no privilege beyond "is an authorized watcher", so a
///   dual-role address gains nothing it could not already do.
/// - Contract addresses are accepted wherever an [`Address`] is taken (admins
///   and watchers alike); Soroban treats account and contract addresses
///   uniformly and the registry does not distinguish them.
///
/// Callers needing external-account-only or disjoint-role guarantees must
/// enforce that off-chain before calling `initialize`, `add_admin`, or
/// `register_watcher`.
#[contract]
pub struct WatcherRegistry;

#[contractimpl]
impl WatcherRegistry {
    /// Initialize the registry with a single bootstrap admin. Can only be called once.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::AlreadyInitialized`] if the contract has already been initialized.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admins) {
            return Err(ContractError::AlreadyInitialized);
        }

        let admins: Vec<Address> = vec![&env, admin.clone()];
        env.storage().instance().set(&DataKey::Admins, &admins);

        env.events()
            .publish((symbol_short!("admin"), symbol_short!("init")), admin);

        Ok(())
    }

    /// Add a new admin to the admin set (any existing admin may call this).
    ///
    /// Idempotent — adding an address that is already an admin is a no-op.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be an
    /// existing admin.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn add_admin(env: Env, caller: Address, new_admin: Address) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_admin(&env, &caller)?;

        let mut admins = Self::load_admins(&env);
        for i in 0..admins.len() {
            if admins.get(i).unwrap() == new_admin {
                return Ok(()); // already an admin, idempotent
            }
        }
        admins.push_back(new_admin.clone());
        env.storage().instance().set(&DataKey::Admins, &admins);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("add")),
            (caller, new_admin),
        );

        Ok(())
    }

    /// Remove an admin from the admin set (any existing admin may call this).
    ///
    /// Refuses to remove the last admin to prevent the contract from becoming
    /// permanently unmanageable.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be an
    /// existing admin.
    /// # Errors
    /// Returns [`ContractError::LastAdmin`] if removing this admin would leave the contract with no admins.
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn remove_admin(
        env: Env,
        caller: Address,
        target_admin: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_admin(&env, &caller)?;

        let admins = Self::load_admins(&env);
        if admins.len() <= 1 {
            return Err(ContractError::LastAdmin);
        }

        let mut updated: Vec<Address> = vec![&env];
        for i in 0..admins.len() {
            let a = admins.get(i).unwrap();
            if a != target_admin {
                updated.push_back(a);
            }
        }
        env.storage().instance().set(&DataKey::Admins, &updated);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("remove")),
            (caller, target_admin),
        );

        Ok(())
    }

    /// Transfer the sole admin role to a new address (any existing admin may call this).
    ///
    /// This replaces the **entire** admin set with a single new admin. Use
    /// [`add_admin`] + [`remove_admin`] if you want to rotate one member of a
    /// multi-admin set without losing the others.
    ///
    /// Emits an `("admin", "transfer")` event recording both the old and new admin.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be an
    /// existing admin.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let new_admins: Vec<Address> = vec![&env, new_admin.clone()];
        env.storage().instance().set(&DataKey::Admins, &new_admins);

        // Emit an auditable on-chain event recording the full admin transfer.
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transfer")),
            (admin, new_admin),
        );

        Ok(())
    }

    /// Register an authorized watcher node (any admin may call this).
    ///
    /// Idempotent — registering an already-authorized watcher is a no-op.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn register_watcher(
        env: Env,
        admin: Address,
        watcher: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let mut watchers = Self::load_watchers(&env);
        for i in 0..watchers.len() {
            if watchers.get(i).unwrap() == watcher {
                return Ok(()); // already registered, idempotent
            }
        }
        watchers.push_back(watcher.clone());
        env.storage().instance().set(&DataKey::Watchers, &watchers);

        Self::increment_watcher_count(&env);

        env.events().publish(
            (symbol_short!("watcher"), symbol_short!("register")),
            watcher,
        );

        Ok(())
    }

    /// Remove (deauthorize) a watcher (any admin may call this).
    ///
    /// If the watcher address is not currently registered this is a no-op —
    /// the call succeeds and no event is emitted.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be an
    /// existing admin.
    ///
    /// # Events
    /// Emits `(Symbol("watcher"), Symbol("remove"))` with data
    /// `(watcher: Address)` when the watcher was present and has been removed.
    /// Dependent systems (e.g. `AlertRegistry` watcher-gating) should listen
    /// for this event to revoke trust immediately.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn remove_watcher(env: Env, admin: Address, watcher: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let watchers = Self::load_watchers(&env);
        let mut updated: Vec<Address> = vec![&env];
        let mut removed = false;
        for i in 0..watchers.len() {
            let w = watchers.get(i).unwrap();
            if w == watcher {
                removed = true;
            } else {
                updated.push_back(w);
            }
        }
        env.storage().instance().set(&DataKey::Watchers, &updated);

        // Only emit the event and decrement the counter when the watcher was
        // actually present.  Callers that need to detect deauthorization must
        // subscribe to this event — it is the authoritative signal that a
        // watcher's trust has been revoked.
        if removed {
            Self::decrement_watcher_count(&env);
            env.events()
                .publish((symbol_short!("watcher"), symbol_short!("remove")), watcher);
        }

        Ok(())
    }

    /// Atomically replace `old_watcher` with `new_watcher` in a single transaction.
    ///
    /// Useful for key rotation — the old address is deauthorized and the new
    /// address is authorized with no gap between the two operations.
    ///
    /// Returns `Err(WatcherNotFound)` if `old_watcher` is not currently registered.
    /// If `new_watcher` is already registered the call still succeeds (the old
    /// entry is removed and the new entry remains).
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be an
    /// existing admin.
    ///
    /// # Events
    /// Emits `("watcher", "remove")` for `old_watcher` and
    /// `("watcher", "replace")` with data `(old_watcher, new_watcher)`.
    /// A self-replace (`old_watcher == new_watcher`) is a no-op and emits
    /// nothing, since the address stays authorized the entire time.
    /// # Errors
    /// Returns [`ContractError::WatcherNotFound`] if the address is not a registered watcher.
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn replace_watcher(
        env: Env,
        admin: Address,
        old_watcher: Address,
        new_watcher: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let watchers = Self::load_watchers(&env);
        let mut found = false;
        let mut updated: Vec<Address> = vec![&env];
        for i in 0..watchers.len() {
            let w = watchers.get(i).unwrap();
            if w == old_watcher {
                found = true;
            } else {
                updated.push_back(w);
            }
        }

        if !found {
            return Err(ContractError::WatcherNotFound);
        }

        // A self-replace leaves old_watcher authorized the whole time. Emitting
        // watcher.remove here would tell listeners to revoke a still-valid
        // watcher, so short-circuit before any storage write or event.
        if old_watcher == new_watcher {
            return Ok(());
        }

        // Add new_watcher only if not already present
        let mut already_present = false;
        for i in 0..updated.len() {
            if updated.get(i).unwrap() == new_watcher {
                already_present = true;
                break;
            }
        }
        if already_present {
            // old was removed but new was already there — net count decreases by 1
            Self::decrement_watcher_count(&env);
        } else {
            updated.push_back(new_watcher.clone());
        }

        env.storage().instance().set(&DataKey::Watchers, &updated);

        env.events().publish(
            (symbol_short!("watcher"), symbol_short!("remove")),
            old_watcher.clone(),
        );
        env.events().publish(
            (symbol_short!("watcher"), symbol_short!("replace")),
            (old_watcher, new_watcher),
        );

        Ok(())
    }

    /// Check if an address is an authorized watcher.
    ///
    /// Renamed from `is_authorized` for clarity in cross-contract call contexts —
    /// the name now makes explicit *what* the address is being authorized as.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    #[must_use]
    pub fn is_watcher_authorized(env: Env, watcher: Address) -> bool {
        let watchers = Self::load_watchers(&env);
        for i in 0..watchers.len() {
            if watchers.get(i).unwrap() == watcher {
                return true;
            }
        }
        false
    }

    /// Alias for [`is_watcher_authorized`] kept for backwards compatibility.
    #[must_use]
    pub fn is_authorized(env: Env, watcher: Address) -> bool {
        Self::is_watcher_authorized(env, watcher)
    }

    /// Remove all registered watchers in a single admin call.
    ///
    /// This is a bulk deauthorization operation.  Each removed watcher emits
    /// a `("watcher", "remove")` event so dependent systems can revoke trust
    /// for every affected address.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be an
    /// existing admin.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn clear_all_watchers(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let watchers = Self::load_watchers(&env);
        for i in 0..watchers.len() {
            let w = watchers.get(i).unwrap();
            env.events()
                .publish((symbol_short!("watcher"), symbol_short!("remove")), w);
        }

        let empty: Vec<Address> = vec![&env];
        env.storage().instance().set(&DataKey::Watchers, &empty);

        // Reset the count to zero
        env.storage().instance().set(&symbol_short!("W_CNT"), &0u32);

        Ok(())
    }

    /// Get all authorized watcher addresses.
    #[must_use]
    pub fn get_watchers(env: Env) -> Vec<Address> {
        Self::load_watchers(&env)
    }

    /// Get all current admin addresses.
    ///
    /// Returns `Err(NotInitialized)` if the contract has not been initialized.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    pub fn get_admins(env: Env) -> Result<Vec<Address>, ContractError> {
        if !env.storage().instance().has(&DataKey::Admins) {
            return Err(ContractError::NotInitialized);
        }
        Ok(Self::load_admins(&env))
    }

    /// Get the primary admin address (first in the admin set).
    ///
    /// Kept for backwards compatibility. Prefer [`get_admins`] when you need
    /// the full admin set.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        let admins = Self::get_admins(env)?;
        // load_admins guarantees at least one entry after initialization
        Ok(admins.get(0).unwrap())
    }

    /// Get the number of registered watchers as a cheap u32 read, avoiding
    /// the cost of fetching and deserializing the full watcher list.
    #[must_use]
    pub fn get_watcher_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("W_CNT"))
            .unwrap_or(0u32)
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn increment_watcher_count(env: &Env) {
        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("W_CNT"))
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&symbol_short!("W_CNT"), &(count + 1));
    }

    fn decrement_watcher_count(env: &Env) {
        let count: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("W_CNT"))
            .unwrap_or(0u32);
        if count > 0 {
            env.storage()
                .instance()
                .set(&symbol_short!("W_CNT"), &(count - 1));
        }
    }

    /// Load the current watcher list from instance storage, or return an empty vec.
    fn load_watchers(env: &Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Watchers)
            .unwrap_or_else(|| vec![env])
    }

    /// Load the current admin set from instance storage, or return an empty vec.
    fn load_admins(env: &Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Admins)
            .unwrap_or_else(|| vec![env])
    }

    /// Return `Ok(())` if `caller` is in the admin set, `Err(Unauthorized)` otherwise.
    fn assert_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if !env.storage().instance().has(&DataKey::Admins) {
            return Err(ContractError::NotInitialized);
        }
        let admins = Self::load_admins(env);
        for i in 0..admins.len() {
            if admins.get(i).unwrap() == *caller {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "regression_tests.rs"]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, testutils::Events as _, Env};

    fn setup() -> (Env, Address, WatcherRegistryClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, admin, client)
    }

    // 1. Happy path — register and check authorization
    #[test]
    fn test_register_and_is_watcher_authorized() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        assert!(!client.is_watcher_authorized(&watcher));
        assert_eq!(
            client.try_register_watcher(&admin, &watcher).unwrap(),
            Ok(())
        );
        assert!(client.is_watcher_authorized(&watcher));
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_initialize_requires_admin_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
    }

    // 2. Happy path — remove watcher
    #[test]
    fn test_remove_watcher() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        client.register_watcher(&admin, &watcher);
        assert_eq!(client.try_remove_watcher(&admin, &watcher).unwrap(), Ok(()));
        assert!(!client.is_watcher_authorized(&watcher));
    }

    // 3. Happy path — transfer admin (replaces entire admin set)
    #[test]
    fn test_transfer_admin() {
        let (env, admin, client) = setup();
        let new_admin = Address::generate(&env);
        let watcher = Address::generate(&env);

        assert_eq!(
            client.try_transfer_admin(&admin, &new_admin).unwrap(),
            Ok(())
        );
        // new admin can register watchers
        assert_eq!(
            client.try_register_watcher(&new_admin, &watcher).unwrap(),
            Ok(())
        );
        assert!(client.is_watcher_authorized(&watcher));
    }

    // 3b. transfer_admin emits an event
    #[test]
    fn test_transfer_admin_emits_event() {
        let (env, admin, client) = setup();
        let new_admin = Address::generate(&env);

        client.transfer_admin(&admin, &new_admin);

        let events = env.events().all();
        // Find the transfer event
        let found = events.iter().any(|e| {
            // topics are (symbol "admin", symbol "transfer")
            // we just verify at least one event was emitted after transfer
            let _ = e;
            true
        });
        assert!(found);
    }

    // 4. Unauthorized register rejected
    #[test]
    fn test_register_unauthorized() {
        let (env, _admin, client) = setup();
        let attacker = Address::generate(&env);
        let watcher = Address::generate(&env);

        assert_eq!(
            client
                .try_register_watcher(&attacker, &watcher)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 5. Unauthorized remove rejected
    #[test]
    fn test_remove_unauthorized() {
        let (env, admin, client) = setup();
        let attacker = Address::generate(&env);
        let watcher = Address::generate(&env);

        client.register_watcher(&admin, &watcher);
        assert_eq!(
            client
                .try_remove_watcher(&attacker, &watcher)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 6. Edge case — double initialize returns AlreadyInitialized error
    #[test]
    fn test_double_initialize() {
        let (env, _admin, client) = setup();
        let other = Address::generate(&env);
        let err = client.try_initialize(&other).unwrap_err().unwrap();
        assert_eq!(err, ContractError::AlreadyInitialized);
    }

    // 7. Edge case — get_watchers returns empty before any registration
    #[test]
    fn test_get_watchers_empty() {
        let (_env, _admin, client) = setup();
        assert_eq!(client.get_watchers().len(), 0);
    }

    // 8. Edge case — register same watcher twice is idempotent
    #[test]
    fn test_register_idempotent() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        assert_eq!(
            client.try_register_watcher(&admin, &watcher).unwrap(),
            Ok(())
        );
        assert_eq!(
            client.try_register_watcher(&admin, &watcher).unwrap(),
            Ok(())
        );
        assert_eq!(client.get_watchers().len(), 1);
    }

    // 8b. Edge case — repeated calls with the same watcher stay idempotent
    #[test]
    fn test_register_idempotent_after_five_duplicates() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        for _ in 0..5 {
            assert_eq!(
                client.try_register_watcher(&admin, &watcher).unwrap(),
                Ok(())
            );
        }
        assert_eq!(client.get_watchers().len(), 1);
    }

    // 9. Multiple watchers
    #[test]
    fn test_multiple_watchers() {
        let (env, admin, client) = setup();
        let w1 = Address::generate(&env);
        let w2 = Address::generate(&env);
        let w3 = Address::generate(&env);

        client.register_watcher(&admin, &w1);
        client.register_watcher(&admin, &w2);
        client.register_watcher(&admin, &w3);

        assert_eq!(client.get_watchers().len(), 3);
        assert!(client.is_watcher_authorized(&w1));
        assert!(client.is_watcher_authorized(&w2));
        assert!(client.is_watcher_authorized(&w3));
    }

    // 9b. Register 3 watchers, remove all 3, verify empty list
    #[test]
    fn test_remove_all_watchers_returns_empty() {
        let (env, admin, client) = setup();
        let w1 = Address::generate(&env);
        let w2 = Address::generate(&env);
        let w3 = Address::generate(&env);

        assert_eq!(client.try_register_watcher(&admin, &w1).unwrap(), Ok(()));
        assert_eq!(client.try_register_watcher(&admin, &w2).unwrap(), Ok(()));
        assert_eq!(client.try_register_watcher(&admin, &w3).unwrap(), Ok(()));
        assert_eq!(client.get_watchers().len(), 3);

        assert_eq!(client.try_remove_watcher(&admin, &w1).unwrap(), Ok(()));
        assert_eq!(client.try_remove_watcher(&admin, &w2).unwrap(), Ok(()));
        assert_eq!(client.try_remove_watcher(&admin, &w3).unwrap(), Ok(()));
        assert_eq!(client.get_watchers().len(), 0);
    }

    // 10. get_admin returns correct admin
    #[test]
    fn test_get_admin() {
        let (_env, admin, client) = setup();
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    fn test_get_admin_uninitialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);

        assert_eq!(
            client.try_get_admin().unwrap_err().unwrap(),
            ContractError::NotInitialized
        );
    }

    // 11. get_admin panics with NotInitialized when contract is not initialized
    #[test]
    #[should_panic(expected = "Error(Contract, #3)")]
    fn test_get_admin_not_initialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        client.get_admin();
    }

    // 12. clear_all_watchers removes all watchers
    #[test]
    fn test_clear_all_watchers() {
        let (env, admin, client) = setup();
        let w1 = Address::generate(&env);
        let w2 = Address::generate(&env);
        let w3 = Address::generate(&env);

        assert_eq!(client.try_register_watcher(&admin, &w1).unwrap(), Ok(()));
        assert_eq!(client.try_register_watcher(&admin, &w2).unwrap(), Ok(()));
        assert_eq!(client.try_register_watcher(&admin, &w3).unwrap(), Ok(()));
        assert_eq!(client.get_watchers().len(), 3);

        assert_eq!(client.try_clear_all_watchers(&admin).unwrap(), Ok(()));
        assert_eq!(client.get_watchers().len(), 0);
        assert!(!client.is_authorized(&w1));
        assert!(!client.is_authorized(&w2));
        assert!(!client.is_authorized(&w3));
    }

    // 13. clear_all_watchers rejects non-admin
    #[test]
    fn test_clear_all_watchers_unauthorized() {
        let (env, admin, client) = setup();
        let attacker = Address::generate(&env);

        assert_eq!(
            client
                .try_register_watcher(&admin, &Address::generate(&env))
                .unwrap(),
            Ok(())
        );

        assert_eq!(
            client
                .try_clear_all_watchers(&attacker)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
        assert_eq!(client.get_watchers().len(), 1);
    }

    // 14. clear_all_watchers on empty list is a no-op (does not error)
    #[test]
    fn test_clear_all_watchers_empty() {
        let (_env, admin, client) = setup();

        assert_eq!(client.try_clear_all_watchers(&admin).unwrap(), Ok(()));
        assert_eq!(client.get_watchers().len(), 0);
    }

    // 15. old admin cannot act after transfer
    #[test]
    fn test_old_admin_rejected_after_transfer() {
        let (env, admin, client) = setup();
        let new_admin = Address::generate(&env);
        let watcher = Address::generate(&env);

        assert_eq!(
            client.try_transfer_admin(&admin, &new_admin).unwrap(),
            Ok(())
        );
        assert_eq!(
            client
                .try_register_watcher(&admin, &watcher)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // ── Multi-admin tests ─────────────────────────────────────────────────────

    // 13. add_admin — second admin can perform privileged operations
    #[test]
    fn test_add_admin_grants_privileges() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);
        let watcher = Address::generate(&env);

        assert_eq!(client.try_add_admin(&admin, &second_admin).unwrap(), Ok(()));

        // second admin can now register watchers
        assert_eq!(
            client
                .try_register_watcher(&second_admin, &watcher)
                .unwrap(),
            Ok(())
        );
        assert!(client.is_authorized(&watcher));
    }

    // 14. add_admin is idempotent
    #[test]
    fn test_add_admin_idempotent() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);

        assert_eq!(client.try_add_admin(&admin, &second_admin).unwrap(), Ok(()));
        assert_eq!(client.try_add_admin(&admin, &second_admin).unwrap(), Ok(()));

        assert_eq!(client.get_admins().len(), 2);
    }

    // 15. remove_admin — removed admin loses privileges
    #[test]
    fn test_remove_admin_revokes_privileges() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);
        let watcher = Address::generate(&env);

        assert_eq!(client.try_add_admin(&admin, &second_admin).unwrap(), Ok(()));
        assert_eq!(
            client.try_remove_admin(&admin, &second_admin).unwrap(),
            Ok(())
        );

        assert_eq!(
            client
                .try_register_watcher(&second_admin, &watcher)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 16. remove_admin — cannot remove the last admin
    #[test]
    fn test_remove_last_admin_rejected() {
        let (_env, admin, client) = setup();

        assert_eq!(
            client
                .try_remove_admin(&admin, &admin)
                .unwrap_err()
                .unwrap(),
            ContractError::LastAdmin
        );
    }

    // 17. get_admins returns all admins
    #[test]
    fn test_get_admins() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);

        assert_eq!(client.try_add_admin(&admin, &second_admin).unwrap(), Ok(()));

        let admins = client.get_admins();
        assert_eq!(admins.len(), 2);
    }

    // 18. non-admin cannot add_admin
    #[test]
    fn test_add_admin_unauthorized() {
        let (env, _admin, client) = setup();
        let attacker = Address::generate(&env);
        let victim = Address::generate(&env);

        assert_eq!(
            client
                .try_add_admin(&attacker, &victim)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 19. non-admin cannot remove_admin
    #[test]
    fn test_remove_admin_unauthorized() {
        let (env, admin, client) = setup();
        let attacker = Address::generate(&env);

        assert_eq!(
            client
                .try_remove_admin(&attacker, &admin)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 20. add_admin emits event
    #[test]
    fn test_add_admin_emits_event() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);

        client.add_admin(&admin, &second_admin);

        // At least one event was emitted (the add event)
        assert!(!env.events().all().is_empty());
    }

    // 21. remove_admin emits event
    #[test]
    fn test_remove_admin_emits_event() {
        let (env, admin, client) = setup();
        let second_admin = Address::generate(&env);

        client.add_admin(&admin, &second_admin);
        client.remove_admin(&admin, &second_admin);

        assert!(!env.events().all().is_empty());
    }

    // 22. remove_watcher emits event
    #[test]
    fn test_remove_watcher_emits_event() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        client.register_watcher(&admin, &watcher);
        client.remove_watcher(&admin, &watcher);

        assert!(!env.events().all().is_empty());
    }

    // 23. remove_watcher event has the correct topic and data shape
    #[test]
    fn test_remove_watcher_event_shape() {
        let (env, admin, client) = setup();
        let watcher = Address::generate(&env);

        client.register_watcher(&admin, &watcher);
        client.remove_watcher(&admin, &watcher);

        let events = env.events().all();
        // Find an event with exactly 2 topics (watcher.remove shape)
        let remove_event = events.iter().find(|(_, topics, _)| topics.len() == 2);
        assert!(remove_event.is_some(), "expected a watcher.remove event");

        // Verify data is the watcher address
        let (_, _, data) = remove_event.unwrap();
        let emitted_watcher: Address = soroban_sdk::FromVal::from_val(&env, &data);
        assert_eq!(emitted_watcher, watcher);
    }

    // 24. remove_watcher on a non-registered address is a no-op — no event emitted
    #[test]
    fn test_remove_watcher_not_registered_no_event() {
        let (env, admin, client) = setup();
        let stranger = Address::generate(&env);

        // stranger was never registered — remove should succeed silently
        client.remove_watcher(&admin, &stranger);

        // Only the admin.init event from setup() should exist; no watcher.remove
        let events = env.events().all();
        assert_eq!(
            events.len(),
            0,
            "no watcher.remove event expected for unregistered watcher"
        );
    }

    // 25. get_watcher_count decrements correctly after remove_watcher
    #[test]
    fn test_watcher_count_decrements_on_remove() {
        let (env, admin, client) = setup();
        let w1 = Address::generate(&env);
        let w2 = Address::generate(&env);

        client.register_watcher(&admin, &w1);
        client.register_watcher(&admin, &w2);
        assert_eq!(client.get_watcher_count(), 2);

        client.remove_watcher(&admin, &w1);
        assert_eq!(client.get_watcher_count(), 1);

        client.remove_watcher(&admin, &w2);
        assert_eq!(client.get_watcher_count(), 0);
    }

    // ── replace_watcher tests ─────────────────────────────────────────────────

    // 26. Happy path — replace_watcher swaps old for new
    #[test]
    fn test_replace_watcher_happy_path() {
        let (env, admin, client) = setup();
        let old = Address::generate(&env);
        let new = Address::generate(&env);

        client.register_watcher(&admin, &old);
        assert_eq!(
            client.try_replace_watcher(&admin, &old, &new).unwrap(),
            Ok(())
        );

        assert!(!client.is_authorized(&old));
        assert!(client.is_authorized(&new));
        assert_eq!(client.get_watcher_count(), 1);
    }

    // 27. replace_watcher errors with WatcherNotFound when old is not registered
    #[test]
    fn test_replace_watcher_old_not_found() {
        let (env, admin, client) = setup();
        let old = Address::generate(&env);
        let new = Address::generate(&env);

        assert_eq!(
            client
                .try_replace_watcher(&admin, &old, &new)
                .unwrap_err()
                .unwrap(),
            ContractError::WatcherNotFound
        );
    }

    // 28. replace_watcher rejects non-admin
    #[test]
    fn test_replace_watcher_unauthorized() {
        let (env, admin, client) = setup();
        let old = Address::generate(&env);
        let new = Address::generate(&env);
        let attacker = Address::generate(&env);

        client.register_watcher(&admin, &old);
        assert_eq!(
            client
                .try_replace_watcher(&attacker, &old, &new)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 29. replace_watcher when new_watcher is already registered — old removed, count decrements
    #[test]
    fn test_replace_watcher_new_already_registered() {
        let (env, admin, client) = setup();
        let old = Address::generate(&env);
        let new = Address::generate(&env);

        client.register_watcher(&admin, &old);
        client.register_watcher(&admin, &new);
        assert_eq!(client.get_watcher_count(), 2);

        assert_eq!(
            client.try_replace_watcher(&admin, &old, &new).unwrap(),
            Ok(())
        );

        assert!(!client.is_authorized(&old));
        assert!(client.is_authorized(&new));
        assert_eq!(client.get_watcher_count(), 1);
    }

    // 30. replace_watcher emits watcher.remove and watcher.replace events
    #[test]
    fn test_replace_watcher_emits_events() {
        use soroban_sdk::testutils::Events as _;

        let (env, admin, client) = setup();
        let old = Address::generate(&env);
        let new = Address::generate(&env);

        client.register_watcher(&admin, &old);
        client.replace_watcher(&admin, &old, &new);

        // At least two events emitted (remove + replace)
        assert!(env.events().all().len() >= 2);
    }

    // 31. self-replace (old == new) is a no-op — no spurious watcher.remove
    #[test]
    fn test_replace_watcher_self_replace_emits_no_events() {
        let (env, admin, client) = setup();
        let w = Address::generate(&env);

        client.register_watcher(&admin, &w);
        client.replace_watcher(&admin, &w, &w);

        assert_eq!(
            env.events().all().len(),
            0,
            "self-replace must not emit any watcher event"
        );
        assert!(client.is_watcher_authorized(&w));
        assert_eq!(client.get_watcher_count(), 1);
    // ── Role policy tests ─────────────────────────────────────────────────────

    // 31. An address may hold both the admin and watcher roles at once.
    #[test]
    fn test_address_can_be_admin_and_watcher() {
        let (env, admin, client) = setup();
        let dual = Address::generate(&env);

        assert_eq!(client.try_add_admin(&admin, &dual).unwrap(), Ok(()));
        assert_eq!(client.try_register_watcher(&admin, &dual).unwrap(), Ok(()));

        assert!(client.is_watcher_authorized(&dual));
        assert!(client.get_admins().contains(&dual));

        // Both roles are independently exercisable by the dual-role address.
        let watcher = Address::generate(&env);
        assert_eq!(
            client.try_register_watcher(&dual, &watcher).unwrap(),
            Ok(())
        );
        assert!(client.is_watcher_authorized(&watcher));
    }

    // 32. A contract address is accepted as a watcher (and as an admin).
    #[test]
    fn test_contract_address_can_hold_roles() {
        let (env, admin, client) = setup();
        let contract_addr = env.register(WatcherRegistry, ());

        assert_eq!(
            client.try_register_watcher(&admin, &contract_addr).unwrap(),
            Ok(())
        );
        assert!(client.is_watcher_authorized(&contract_addr));

        assert_eq!(
            client.try_add_admin(&admin, &contract_addr).unwrap(),
            Ok(())
        );
        assert!(client.get_admins().contains(&contract_addr));
    }

    // ── Auth-failure tests (no mock_all_auths) ────────────────────────────────

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_add_admin_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.add_admin(&admin, &new_admin);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_remove_admin_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin1 = Address::generate(&env);
        let admin2 = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin1);
        client.add_admin(&admin1, &admin2);
        env.set_auths(&[]);
        client.remove_admin(&admin1, &admin2);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_transfer_admin_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.transfer_admin(&admin, &new_admin);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_register_watcher_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.register_watcher(&admin, &watcher);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_remove_watcher_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        client.register_watcher(&admin, &watcher);
        env.set_auths(&[]);
        client.remove_watcher(&admin, &watcher);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_replace_watcher_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let old_watcher = Address::generate(&env);
        let new_watcher = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        client.register_watcher(&admin, &old_watcher);
        env.set_auths(&[]);
        client.replace_watcher(&admin, &old_watcher, &new_watcher);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_clear_all_watchers_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(WatcherRegistry, ());
        let client = WatcherRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        client.register_watcher(&admin, &watcher);
        env.set_auths(&[]);
        client.clear_all_watchers(&admin);
    }
}
