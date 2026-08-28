#![no_std]
#![warn(clippy::pedantic)]
// Soroban's generated contract interface dictates these shapes, so the
// corresponding pedantic lints fire on correct code and are scoped off here
// rather than silenced case by case:
//   - contract entry points must take `Env` and `Address` by value
//   - `#[contractimpl]` re-exports getters, so `#[must_use]` is not ours to add
#![allow(clippy::needless_pass_by_value, clippy::must_use_candidate)]
use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, panic_with_error,
    symbol_short, vec, Address, Env, String, Vec,
};

contractmeta!(key = "Name", val = "AlertRegistry");
contractmeta!(key = "Version", val = "0.1.0");

// ── Storage keys ────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests.rs"]
mod contract_tests;

// ── TTL constants ─────────────────────────────────────────────────────────────

/// Default TTL applied to persistent storage entries on every write.
///
/// Approximately 24 hours at the nominal 5-second ledger close time.
/// See `docs/ttl.md` for the full rationale.
pub const DEFAULT_TTL: u32 = 17_280;

/// Protocol-enforced upper bound on caller-specified TTL values.
///
/// Callers may request any TTL up to this value when calling
/// [`AlertRegistry::bump_alert`].  Requests above this cap are silently
/// clamped to `MAX_TTL`.
///
/// Approximately 31 days at the nominal 5-second ledger close time.
pub const MAX_TTL: u32 = 535_680;

/// Storage key variants used to address persistent and instance entries.
#[contracttype]
pub enum DataKey {
    /// Stores an [`AlertConfig`] keyed by its numeric ID.
    Alert(u64),
    /// Stores just the `active` bool separately so it can be read without
    /// deserializing the full [`AlertConfig`].
    AlertActive(u64),
    /// Stores the list of alert IDs owned by a given address.
    OwnerIndex(Address),
    /// Stores the list of alert IDs watching a given contract address.
    ContractIndex(Address),
    /// Monotonic counter used to generate unique alert IDs.
    NextId,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    AlertNotFound = 2,
    AlreadyInitialized = 3,
    NotInitialized = 4,
    /// Returned when a watcher registry is configured and the querying address
    /// is not a registered watcher.
    NotAWatcher = 5,
    InvalidWebhookHash = 6,
    LabelTooLong = 7,
    TooManyRules = 8,
    InvalidRuleDescriptor = 9,
    OwnerAlertLimitExceeded = 10,
    DuplicateAlertId = 11,
    /// Returned by `confirm_webhook` when no webhook rotation is in progress.
    NoPendingWebhook = 12,
    /// Returned when a state-mutating call is made while the contract is paused.
    Paused = 13,
}

// ── Data types ───────────────────────────────────────────────────────────────

/// On-chain configuration for a single alert.
///
/// Stored under [`DataKey::Alert`] with a default TTL of [`DEFAULT_TTL`] ledgers
/// (~24 hours). Use [`AlertRegistry::bump_alert`] to extend up to [`MAX_TTL`].
/// See `docs/ttl.md` for expiry details.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AlertConfig {
    /// Human-readable label for the alert.
    pub label: String,
    /// SHA-256 hash of the webhook URL (the raw URL is never stored on-chain).
    pub webhook_hash: String,
    /// Staged replacement for `webhook_hash` during a two-phase rotation.
    ///
    /// Set by [`AlertRegistry::propose_webhook`] and promoted to `webhook_hash`
    /// by [`AlertRegistry::confirm_webhook`]. `None` when no rotation is in
    /// progress. Staging the change means a misconfigured endpoint never
    /// silently replaces a working one.
    pub pending_webhook_hash: Option<String>,
    /// List of rule identifiers that trigger this alert (e.g. `"rule:transfer"`).
    pub rules: Vec<String>,
    /// Address that owns and may mutate this alert.
    pub owner: Address,
    /// Contract address being watched.
    pub target_contract: Address,
    /// Ledger timestamp at the time of registration.
    pub created_at: u64,
    /// Ledger timestamp of the most recent update.
    pub updated_at: u64,
    /// Whether the alert is currently active.
    pub active: bool,
}

// ── Contract ─────────────────────────────────────────────────────────────────

/// On-chain registry for alert configurations.
///
/// Each alert is keyed by a monotonically increasing `u64` ID and indexed by
/// both owner address and target contract address for efficient lookups.
///
/// # Watcher-gating (optional)
/// When a `WatcherRegistry` contract address is configured via
/// [`set_watcher_registry`], the read-only query functions
/// (`get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated
/// variants) will perform a cross-contract call to verify that the querying
/// address is a registered watcher before returning data. Callers that are not
/// registered watchers receive [`ContractError::NotAWatcher`].
///
/// If no watcher registry is configured the gating is skipped and the
/// functions behave as before.
///
/// # Storage and TTL
/// All persistent entries are extended by [`DEFAULT_TTL`] ledgers (~24 hours) on every
/// write. Callers can extend any alert up to [`MAX_TTL`] ledgers (~31 days) via
/// [`bump_alert`]. See `docs/ttl.md` for full details.
#[contract]
pub struct AlertRegistry;

// ── Cross-contract interface for WatcherRegistry ─────────────────────────────

/// Minimal client interface for calling `WatcherRegistry::is_watcher_authorized`
/// from within `AlertRegistry`.
mod watcher_registry_interface {
    use soroban_sdk::{contractclient, Address, Env};

    #[allow(dead_code)]
    #[contractclient(name = "WatcherRegistryClient")]
    pub trait WatcherRegistry {
        fn is_watcher_authorized(env: Env, watcher: Address) -> bool;
    }
}

use watcher_registry_interface::WatcherRegistryClient as ExtWatcherClient;

#[contractimpl]
impl AlertRegistry {
    // ── Admin / configuration ─────────────────────────────────────────────

    /// Initialize the optional admin role for the registry. Can only be called once.
    /// # Errors
    /// Returns [`ContractError::AlreadyInitialized`] if the contract has already been initialized.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&symbol_short!("ADMIN")) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&symbol_short!("ADMIN"), &admin);
        Ok(())
    }

    /// Transfer the admin role to a new address (admin only).
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
        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("ADMIN"), &new_admin);

        // Admin handover is security-relevant: emit it so off-chain watchers
        // can react to a change of control.
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transfer")),
            (admin, new_admin),
        );
        Ok(())
    }

    /// Get the current admin address.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&symbol_short!("ADMIN"))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::NotInitialized))
    }

    /// Pause the contract, rejecting all state-mutating calls until [`Self::unpause`] is called.
    ///
    /// Intended as an emergency circuit-breaker if an admin key is suspected
    /// compromised — mutations can be frozen while the incident is investigated.
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&symbol_short!("PAUSED"), &true);
        env.events()
            .publish((symbol_short!("admin"), symbol_short!("pause")), admin);
        Ok(())
    }

    /// Resume normal operation after a [`Self::pause`].
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&symbol_short!("PAUSED"), &false);
        env.events()
            .publish((symbol_short!("admin"), symbol_short!("unpause")), admin);
        Ok(())
    }

    /// Return `true` if the contract is currently paused.
    #[must_use]
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&symbol_short!("PAUSED"))
            .unwrap_or(false)
    }

    /// Set a per-owner active alert limit (admin only). A value of `0` means no limit.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn set_per_owner_alert_limit(
        env: Env,
        admin: Address,
        limit: u32,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("LIMIT"), &limit);
        Ok(())
    }

    /// Get the configured per-owner active alert limit, or `0` if none is set.
    pub fn get_per_owner_alert_limit(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("LIMIT"))
            .unwrap_or(0u32)
    }

    /// Configure the `WatcherRegistry` contract address used for optional
    /// watcher-gating on read queries (admin only).
    ///
    /// Once set, `get_alerts_for_contract`, `get_alerts_by_owner`, and their
    /// paginated variants will cross-call `WatcherRegistry::is_watcher_authorized`
    /// before returning data. Pass the zero address to disable gating.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn set_watcher_registry(
        env: Env,
        admin: Address,
        watcher_registry: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("WATCHREG"), &watcher_registry);
        Ok(())
    }

    /// Return the configured `WatcherRegistry` contract address, or `None` if
    /// watcher-gating has not been enabled.
    pub fn get_watcher_registry(env: Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("WATCHREG"))
    }

    /// Return `true` if watcher-gating is currently enabled (a `WatcherRegistry`
    /// contract address is configured), `false` otherwise.
    ///
    /// # Arguments
    /// * `env` - Soroban environment.
    ///
    /// # Returns
    /// `true` if a watcher registry address is set, `false` otherwise.
    pub fn is_watcher_gating_enabled(env: Env) -> bool {
        Self::get_watcher_registry(env).is_some()
    }

    // ── Alert mutations ───────────────────────────────────────────────────

    /// Register a new alert config and return its assigned ID.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `owner`.
    ///
    /// # Arguments
    /// * `owner` - Address that will own and control this alert.
    /// * `target_contract` - Contract address to watch.
    /// * `label` - Human-readable name for the alert.
    /// * `webhook_hash` - SHA-256 hash of the destination webhook URL.
    /// * `rules` - Rule identifiers that should trigger the alert.
    ///
    /// # Returns
    /// The new alert's numeric ID.
    /// # Errors
    /// Returns [`ContractError::InvalidWebhookHash`] if `webhook_hash` is not exactly 64 characters.
    /// Returns [`ContractError::LabelTooLong`] if `label` exceeds 128 bytes.
    /// Returns [`ContractError::OwnerAlertLimitExceeded`] if the owner is at the configured per-owner alert limit.
    /// Returns [`ContractError::TooManyRules`] if `rules` exceeds the 50-rule maximum.
    /// Returns [`ContractError::InvalidRuleDescriptor`] if a rule is not a recognised descriptor.
    pub fn register_alert(
        env: Env,
        owner: Address,
        target_contract: Address,
        label: String,
        webhook_hash: String,
        rules: Vec<String>,
    ) -> Result<u64, ContractError> {
        if webhook_hash.len() != 64 {
            return Err(ContractError::InvalidWebhookHash);
        }
        owner.require_auth();
        Self::assert_not_paused(&env)?;

        if label.len() > 128 {
            return Err(ContractError::LabelTooLong);
        }

        Self::validate_rules(&env, &rules)?;
        Self::assert_per_owner_limit(&env, &owner)?;

        let id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let config = AlertConfig {
            label,
            webhook_hash,
            pending_webhook_hash: None,
            rules,
            owner: owner.clone(),
            target_contract: target_contract.clone(),
            created_at: now,
            updated_at: now,
            active: true,
        };

        env.storage().persistent().set(&DataKey::Alert(id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(id), DEFAULT_TTL, DEFAULT_TTL);
        env.storage()
            .persistent()
            .set(&DataKey::AlertActive(id), &true);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::AlertActive(id), DEFAULT_TTL, DEFAULT_TTL);
        Self::push_owner_index(&env, &owner, id)?;
        Self::push_contract_index(&env, &target_contract, id)?;

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("register")),
            (id, owner, target_contract),
        );

        Ok(id)
    }

    /// Update the rules and active flag of an existing alert.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// Returns [`ContractError::TooManyRules`] if `rules` exceeds the 50-rule maximum.
    /// Returns [`ContractError::InvalidRuleDescriptor`] if a rule is not a recognised descriptor.
    pub fn update_alert(
        env: Env,
        caller: Address,
        config_id: u64,
        rules: Vec<String>,
        active: bool,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;
        Self::validate_rules(&env, &rules)?;

        config.rules = rules;
        config.active = active;
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);
        // Keep the cheap AlertActive flag in sync with the full config.
        env.storage()
            .persistent()
            .set(&DataKey::AlertActive(config_id), &active);
        env.storage().persistent().extend_ttl(
            &DataKey::AlertActive(config_id),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(config.owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(config.target_contract.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Update the webhook hash for an existing alert.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    /// # Errors
    /// Returns [`ContractError::InvalidWebhookHash`] if `webhook_hash` is not exactly 64 characters.
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn update_webhook(
        env: Env,
        caller: Address,
        config_id: u64,
        webhook_hash: String,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        if webhook_hash.len() != 64 {
            return Err(ContractError::InvalidWebhookHash);
        }

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        config.webhook_hash = webhook_hash;
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(config.owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(config.target_contract.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Stage a replacement webhook hash without taking it live.
    ///
    /// The alert keeps delivering to its current `webhook_hash` until
    /// [`Self::confirm_webhook`] promotes the staged value, so a mistyped or
    /// unreachable endpoint can never silently displace a working one. Calling
    /// this again before confirming overwrites the staged value.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// alert owner.
    ///
    /// # Errors
    /// Returns [`ContractError::InvalidWebhookHash`] unless `webhook_hash` is
    /// exactly 64 characters.
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the owner.
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("wh_prop"))` with data `(id: u64, caller: Address)`.
    pub fn propose_webhook(
        env: Env,
        caller: Address,
        config_id: u64,
        webhook_hash: String,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        if webhook_hash.len() != 64 {
            return Err(ContractError::InvalidWebhookHash);
        }

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        // The live hash is deliberately left untouched until confirmation.
        config.pending_webhook_hash = Some(webhook_hash);

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("wh_prop")),
            (config_id, caller),
        );
        Ok(())
    }

    /// Promote the staged webhook hash to the live one, completing a rotation.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// alert owner.
    ///
    /// # Errors
    /// Returns [`ContractError::NoPendingWebhook`] if no rotation is in
    /// progress.
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the owner.
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("wh_conf"))` with data `(id: u64, caller: Address)`.
    pub fn confirm_webhook(env: Env, caller: Address, config_id: u64) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        let pending = config
            .pending_webhook_hash
            .clone()
            .ok_or(ContractError::NoPendingWebhook)?;

        config.webhook_hash = pending;
        config.pending_webhook_hash = None;
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("wh_conf")),
            (config_id, caller),
        );
        Ok(())
    }

    /// Extend the TTL of an alert and its indexes without modifying any data.
    ///
    /// Unlike [`Self::bump_alert`], this is owner-authenticated and leaves
    /// `updated_at` alone, so renewing storage never looks like an edit to
    /// downstream consumers polling `get_alerts_modified_since`.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// alert owner.
    ///
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the owner.
    pub fn renew_alert_ttl(env: Env, caller: Address, config_id: u64) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);
        env.storage().persistent().extend_ttl(
            &DataKey::AlertActive(config_id),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(config.owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(config.target_contract.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Update only the label of an existing alert, leaving rules and webhook hash unchanged.
    ///
    /// Use this when you want to rename an alert without touching its rules or
    /// rotating its webhook URL.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    ///
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the alert owner.
    ///
    /// # Panics
    /// Panics if `label` exceeds 128 bytes.
    pub fn update_label(
        env: Env,
        caller: Address,
        config_id: u64,
        label: String,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        if label.len() > 128 {
            return Err(ContractError::LabelTooLong);
        }

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        config.label = label;
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(config.owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(config.target_contract.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Remove an alert config from storage.
    ///
    /// Also removes the alert ID from the owner and contract indexes.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn remove_alert(env: Env, caller: Address, config_id: u64) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;
        Self::remove_alert_record(&env, &config, config_id, &caller);
        Ok(())
    }

    /// Remove any alert config from storage (admin only).
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn remove_alert_by_admin(
        env: Env,
        admin: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::assert_not_paused(&env)?;

        let config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::remove_alert_record(&env, &config, config_id, &admin);
        Ok(())
    }

    /// Extend the TTL of an alert and its associated indexes.
    ///
    /// Callers may request any TTL up to [`MAX_TTL`] ledgers.  Values above
    /// the cap are silently clamped to `MAX_TTL`, so callers can safely pass
    /// `u32::MAX` to request the longest possible lifetime.
    ///
    /// This is the primary mechanism for keeping long-lived alerts alive
    /// without modifying their content.  Unlike `update_alert`, this function
    /// does **not** require the caller to be the alert owner — any address may
    /// bump an alert's TTL (e.g. an off-chain keeper service).
    ///
    /// # Arguments
    /// * `config_id` - ID of the alert to extend.
    /// * `ttl`       - Desired TTL in ledgers (clamped to [`MAX_TTL`]).
    ///
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("bump"))` with data
    /// `(id: u64, ttl: u32)` so off-chain indexers can track renewal activity.
    pub fn bump_alert(env: Env, config_id: u64, ttl: u32) -> Result<(), ContractError> {
        Self::assert_not_paused(&env)?;

        // Clamp the requested TTL to the protocol maximum.
        let effective_ttl = ttl.min(MAX_TTL);

        let config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        env.storage().persistent().extend_ttl(
            &DataKey::Alert(config_id),
            effective_ttl,
            effective_ttl,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::AlertActive(config_id),
            effective_ttl,
            effective_ttl,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(config.owner.clone()),
            effective_ttl,
            effective_ttl,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(config.target_contract),
            effective_ttl,
            effective_ttl,
        );

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("bump")),
            (config_id, effective_ttl),
        );

        Ok(())
    }

    /// Retrieve all alert configs that watch a given contract address.
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    ///
    /// Returns an empty vec if no alerts exist for `target_contract`.
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alerts_for_contract(
        env: Env,
        querier: Address,
        target_contract: Address,
    ) -> Result<Vec<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        let ids = Self::contract_index(&env, &target_contract);
        Ok(Self::configs_for_ids(&env, &ids))
    }

    /// Retrieve only the active alert configs that watch a given contract address.
    ///
    /// Equivalent to [`get_alerts_for_contract`] but filters out any entries
    /// where `active == false`. Returns an empty vec if no active alerts exist
    /// for `target_contract`.
    pub fn get_active_alerts_for_contract(env: Env, target_contract: Address) -> Vec<AlertConfig> {
        let ids = Self::contract_index(&env, &target_contract);
        Self::active_configs_for_ids(&env, &ids)
    }

    /// Retrieve all alert configs owned by a given address.
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    ///
    /// Returns an empty vec if `owner` has no registered alerts.
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alerts_by_owner(
        env: Env,
        querier: Address,
        owner: Address,
    ) -> Result<Vec<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        let ids = Self::owner_index(&env, &owner);
        Ok(Self::configs_for_ids(&env, &ids))
    }

    /// Get a page of alert configs for a target contract (offset + limit).
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_contract_alerts_paginated(
        env: Env,
        querier: Address,
        target_contract: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        let ids = Self::contract_index(&env, &target_contract);
        Ok(Self::configs_paginated(&env, &ids, offset, limit))
    }

    /// Get a page of alert configs owned by an address (offset + limit).
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alerts_by_owner_paginated(
        env: Env,
        querier: Address,
        owner: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        let ids = Self::owner_index(&env, &owner);
        Ok(Self::configs_paginated(&env, &ids, offset, limit))
    }

    /// Retrieve a single alert config by its ID.
    ///
    /// Returns `None` if the alert does not exist or has expired.
    pub fn get_alert(env: Env, config_id: u64) -> Option<AlertConfig> {
        env.storage().persistent().get(&DataKey::Alert(config_id))
    }

    /// Read the `active` flag of an alert without deserializing the full config.
    ///
    /// Returns `None` if the alert does not exist or has expired.
    pub fn get_alert_active(env: Env, config_id: u64) -> Option<bool> {
        env.storage()
            .persistent()
            .get(&DataKey::AlertActive(config_id))
    }

    /// Deactivate all alerts owned by `caller` in a single call.
    ///
    /// Iterates the owner's index and sets `active = false` on every live
    /// alert.  Expired or already-removed entries are silently skipped.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`.
    ///
    /// # Returns
    /// The number of alerts that were deactivated.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn deactivate_all_alerts(env: Env, caller: Address) -> u32 {
        caller.require_auth();
        if Self::is_paused(env.clone()) {
            return 0;
        }
        let ids = Self::owner_index(&env, &caller);
        let mut count: u32 = 0;
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(mut cfg) = env
                .storage()
                .persistent()
                .get::<DataKey, AlertConfig>(&DataKey::Alert(id))
            {
                if cfg.active {
                    cfg.active = false;
                    cfg.updated_at = env.ledger().timestamp();
                    env.storage().persistent().set(&DataKey::Alert(id), &cfg);
                    env.storage().persistent().extend_ttl(
                        &DataKey::Alert(id),
                        DEFAULT_TTL,
                        DEFAULT_TTL,
                    );
                    env.storage()
                        .persistent()
                        .set(&DataKey::AlertActive(id), &false);
                    env.storage().persistent().extend_ttl(
                        &DataKey::AlertActive(id),
                        DEFAULT_TTL,
                        DEFAULT_TTL,
                    );
                    count += 1;
                }
            }
        }
        count
    }

    /// Move an alert to watch a different target contract.
    ///
    /// Updates the `target_contract` field of the alert config and migrates
    /// the alert ID from the old contract index to the new one.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    ///
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not exist.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the alert owner.
    pub fn update_target_contract(
        env: Env,
        caller: Address,
        config_id: u64,
        new_target: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        let old_target = config.target_contract.clone();
        config.target_contract = new_target.clone();
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);

        // Migrate the contract index
        Self::remove_from_contract_index(&env, &old_target, config_id);
        Self::push_contract_index(&env, &new_target, config_id)?;

        Ok(())
    }

    /// Return all alert configs whose `updated_at` timestamp is greater than or
    /// equal to `since`.
    ///
    /// This enables efficient **incremental sync** for watcher nodes: on each
    /// polling cycle a watcher passes the ledger timestamp of its last sync and
    /// receives only the alerts that have been created or modified since then,
    /// rather than fetching the entire registry.
    ///
    /// # Arguments
    /// * `since` - Ledger timestamp (inclusive lower bound). Pass `0` to
    ///   retrieve every alert that is currently stored.
    ///
    /// # Returns
    /// A `Vec<AlertConfig>` containing every live alert with
    /// `updated_at >= since`. Alerts that have been removed (and whose storage
    /// entry has therefore expired) are silently omitted.
    ///
    /// # Note
    /// The function scans all IDs from `0` up to the current `NEXT_ID`
    /// counter. This is acceptable for the expected registry sizes on Soroban
    /// and avoids the need for a separate timestamp index. For very large
    /// registries consider combining this with the paginated variants.
    #[must_use]
    pub fn get_alerts_modified_since(env: Env, since: u64) -> Vec<AlertConfig> {
        let total: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u64);

        let mut out: Vec<AlertConfig> = vec![&env];
        for id in 0..total {
            if let Some(cfg) = env
                .storage()
                .persistent()
                .get::<DataKey, AlertConfig>(&DataKey::Alert(id))
            {
                if cfg.updated_at >= since {
                    out.push_back(cfg);
                }
            }
        }
        out
    }

    /// Get the total number of alerts ever registered.
    ///
    /// This is a **monotonic counter** — it only increases and is never
    /// decremented when alerts are removed. Use [`get_active_alert_count`]
    /// if you need the number of currently live alerts for a given owner.
    #[must_use]
    pub fn get_alert_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u64)
    }

    /// Get the number of currently active (non-removed) alerts owned by `owner`.
    ///
    /// Unlike [`get_alert_count`], this reflects removals and only counts
    /// alerts whose storage entries are still live.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn get_active_alert_count(env: Env, owner: Address) -> u32 {
        let ids = Self::owner_index(&env, &owner);
        let mut count: u32 = 0;
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if env.storage().persistent().has(&DataKey::Alert(id)) {
                count += 1;
            }
        }
        count
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    /// If a `WatcherRegistry` contract address is stored in instance storage,
    /// perform a cross-contract call to verify that `querier` is a registered
    /// watcher. Returns `Ok(())` when no registry is configured (gating is
    /// disabled) or when the querier passes the check.
    fn assert_watcher_if_configured(env: &Env, querier: &Address) -> Result<(), ContractError> {
        let maybe_registry: Option<Address> =
            env.storage().instance().get(&symbol_short!("WATCHREG"));

        if let Some(registry_addr) = maybe_registry {
            let client = ExtWatcherClient::new(env, &registry_addr);
            if !client.is_watcher_authorized(querier) {
                return Err(ContractError::NotAWatcher);
            }
        }
        Ok(())
    }

    fn assert_owner(config: &AlertConfig, caller: &Address) -> Result<(), ContractError> {
        if config.owner == *caller {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn assert_not_paused(env: &Env) -> Result<(), ContractError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&symbol_short!("PAUSED"))
            .unwrap_or(false);
        if paused {
            return Err(ContractError::Paused);
        }
        Ok(())
    }

    fn assert_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if !env.storage().instance().has(&symbol_short!("ADMIN")) {
            return Err(ContractError::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&symbol_short!("ADMIN"))
            .unwrap();
        if admin == *caller {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn assert_per_owner_limit(env: &Env, owner: &Address) -> Result<(), ContractError> {
        let limit = Self::get_per_owner_alert_limit(env.clone());
        if limit > 0 && Self::get_active_alert_count(env.clone(), owner.clone()) >= limit {
            return Err(ContractError::OwnerAlertLimitExceeded);
        }
        Ok(())
    }

    fn validate_rules(env: &Env, rules: &Vec<String>) -> Result<(), ContractError> {
        if rules.len() > 50 {
            return Err(ContractError::TooManyRules);
        }
        for i in 0..rules.len() {
            Self::validate_rule(env, &rules.get(i).unwrap())?;
        }
        Ok(())
    }

    fn validate_rule(env: &Env, rule: &String) -> Result<(), ContractError> {
        let transfer = String::from_str(env, "rule:transfer");
        let mint = String::from_str(env, "rule:mint");
        if *rule != transfer && *rule != mint {
            return Err(ContractError::InvalidRuleDescriptor);
        }
        Ok(())
    }

    fn remove_alert_record(env: &Env, config: &AlertConfig, config_id: u64, caller: &Address) {
        env.storage()
            .persistent()
            .remove(&DataKey::Alert(config_id));
        env.storage()
            .persistent()
            .remove(&DataKey::AlertActive(config_id));

        Self::remove_from_owner_index(env, &config.owner, config_id);
        Self::remove_from_contract_index(env, &config.target_contract, config_id);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("remove")),
            (config_id, caller.clone()),
        );
    }

    /// Atomically read and increment the global alert ID counter.
    ///
    /// Returns the current value before incrementing, so the first ID is `0`.
    fn next_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u64);
        env.storage()
            .instance()
            .set(&symbol_short!("NEXT_ID"), &(id + 1));
        id
    }

    /// Load the list of alert IDs owned by `owner`, or an empty vec.
    fn owner_index(env: &Env, owner: &Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerIndex(owner.clone()))
            .unwrap_or_else(|| vec![env])
    }

    /// Load the list of alert IDs watching `target`, or an empty vec.
    fn contract_index(env: &Env, target: &Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::ContractIndex(target.clone()))
            .unwrap_or_else(|| vec![env])
    }

    /// Append `id` to the owner's index and persist it with a refreshed TTL.
    fn push_owner_index(env: &Env, owner: &Address, id: u64) -> Result<(), ContractError> {
        let mut ids = Self::owner_index(env, owner);
        for i in 0..ids.len() {
            if ids.get(i).unwrap() == id {
                return Err(ContractError::DuplicateAlertId);
            }
        }
        ids.push_back(id);
        env.storage()
            .persistent()
            .set(&DataKey::OwnerIndex(owner.clone()), &ids);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Append `id` to the contract's index and persist it with a refreshed TTL.
    fn push_contract_index(env: &Env, target: &Address, id: u64) -> Result<(), ContractError> {
        let mut ids = Self::contract_index(env, target);
        for i in 0..ids.len() {
            if ids.get(i).unwrap() == id {
                return Err(ContractError::DuplicateAlertId);
            }
        }
        ids.push_back(id);
        env.storage()
            .persistent()
            .set(&DataKey::ContractIndex(target.clone()), &ids);
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(target.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
        Ok(())
    }

    /// Remove `id` from the owner's index and persist the updated list.
    fn remove_from_owner_index(env: &Env, owner: &Address, id: u64) {
        let ids = Self::owner_index(env, owner);
        let mut updated: Vec<u64> = vec![env];
        for i in 0..ids.len() {
            let v = ids.get(i).unwrap();
            if v != id {
                updated.push_back(v);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::OwnerIndex(owner.clone()), &updated);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
    }

    /// Remove `id` from the contract's index and persist the updated list.
    fn remove_from_contract_index(env: &Env, target: &Address, id: u64) {
        let ids = Self::contract_index(env, target);
        let mut updated: Vec<u64> = vec![env];
        for i in 0..ids.len() {
            let v = ids.get(i).unwrap();
            if v != id {
                updated.push_back(v);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::ContractIndex(target.clone()), &updated);
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(target.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
    }

    /// Resolve a list of alert IDs to their stored [`AlertConfig`] values.
    ///
    /// IDs that no longer exist in storage (expired or removed) are silently
    /// skipped.
    fn configs_for_ids(env: &Env, ids: &Vec<u64>) -> Vec<AlertConfig> {
        let mut out: Vec<AlertConfig> = vec![env];
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(cfg) = env.storage().persistent().get(&DataKey::Alert(id)) {
                out.push_back(cfg);
            }
        }
        out
    }

    /// Like [`configs_for_ids`] but only includes entries where `active == true`.
    ///
    /// IDs that no longer exist in storage are silently skipped, as are configs
    /// whose `active` field is `false`.
    fn active_configs_for_ids(env: &Env, ids: &Vec<u64>) -> Vec<AlertConfig> {
        let mut out: Vec<AlertConfig> = vec![env];
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(cfg) = env
                .storage()
                .persistent()
                .get::<DataKey, AlertConfig>(&DataKey::Alert(id))
            {
                if cfg.active {
                    out.push_back(cfg);
                }
            }
        }
        out
    }

    fn configs_paginated(env: &Env, ids: &Vec<u64>, offset: u32, limit: u32) -> Vec<AlertConfig> {
        let mut out: Vec<AlertConfig> = vec![env];
        let count = ids.len();
        let first = offset.min(count);
        let last = offset.saturating_add(limit).min(count);
        for i in first..last {
            let id = ids.get(i).unwrap();
            if let Some(cfg) = env.storage().persistent().get(&DataKey::Alert(id)) {
                out.push_back(cfg);
            }
        }
        out
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _, Ledger as _},
        vec, Env, FromVal, String, Symbol,
    };

    /// A 64-character webhook hash of repeated `c` — `register_alert`,
    /// `update_webhook` and `propose_webhook` all require exactly 64 characters.
    fn hash64c(env: &Env, c: char) -> String {
        let buf = [c as u8; 64];
        String::from_str(env, core::str::from_utf8(&buf).unwrap())
    }

    /// The default valid 64-character webhook hash.
    fn hash64(env: &Env) -> String {
        hash64c(env, '0')
    }

    fn setup() -> (Env, AlertRegistryClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        (env, client)
    }

    fn str(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    // ── Helpers shared by watcher-gating tests ────────────────────────────

    #[cfg(feature = "testutils")]
    fn setup_with_watcher_registry() -> (
        Env,
        AlertRegistryClient<'static>,
        watcher_registry::WatcherRegistryClient<'static>,
    ) {
        use watcher_registry::WatcherRegistry;
        let env = Env::default();
        env.mock_all_auths();

        let alert_id = env.register(AlertRegistry, ());
        let watcher_id = env.register(WatcherRegistry, ());

        let alert_client = AlertRegistryClient::new(&env, &alert_id);
        let watcher_client = watcher_registry::WatcherRegistryClient::new(&env, &watcher_id);

        (env, alert_client, watcher_client)
    }

    // 1. Happy path — register and retrieve
    #[test]
    fn test_register_and_get_alert() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "My Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.label, str(&env, "My Alert"));
        assert_eq!(cfg.owner, owner);
        assert!(cfg.active);
    }

    // 2. Happy path — update alert
    #[test]
    fn test_update_alert() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        assert_eq!(
            client
                .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false)
                .unwrap(),
            Ok(())
        );

        let cfg = client.get_alert(&id).unwrap();
        assert!(!cfg.active);
        assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:mint"));
    }

    // 3. Happy path — remove alert
    #[test]
    fn test_remove_alert() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.try_remove_alert(&owner, &id).unwrap(), Ok(()));
        assert!(client.get_alert(&id).is_none());
    }

    // 4. Unauthorized update rejected
    #[test]
    fn test_update_unauthorized() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(
            client
                .try_update_alert(&attacker, &id, &vec![&env], &false)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_register_alert_rejects_invalid_rules() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:unknown")],
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_update_alert_rejects_invalid_rules() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:bogus")], &true);
    }

    #[test]
    fn test_admin_remove_any_alert() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:mint")],
        );

        client.remove_alert_by_admin(&admin, &id);
        assert!(client.get_alert(&id).is_none());
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #10)")]
    fn test_admin_set_per_owner_alert_limit() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_per_owner_alert_limit(&admin, &1u32);

        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );

        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );
    }

    #[test]
    fn test_admin_transfer_admin() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        client.transfer_admin(&admin, &new_admin);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        client.remove_alert_by_admin(&new_admin, &id);
    }

    #[test]
    fn test_old_admin_rejected_after_transfer() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        // first transfer succeeds
        assert_eq!(
            client.try_transfer_admin(&admin, &new_admin).unwrap(),
            Ok(())
        );

        // old admin cannot call transfer_admin again
        assert_eq!(
            client
                .try_transfer_admin(&admin, &new_admin)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 5. Unauthorized remove rejected
    #[test]
    fn test_remove_unauthorized() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(
            client
                .try_remove_alert(&attacker, &id)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // Issue #49 — get_alert_count is monotonically increasing after multiple register/remove cycles
    #[test]
    fn test_get_alert_count_after_multiple_cycles() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Start at 0
        assert_eq!(client.get_alert_count(), 0);

        // Cycle 1: register -> count goes to 1
        let id1 =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        assert_eq!(client.get_alert_count(), 1);
        // remove -> count stays at 1 (monotonic)
        client.remove_alert(&owner, &id1);
        assert_eq!(client.get_alert_count(), 1);

        // Cycle 2: register -> count goes to 2
        let id2 =
            client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
        assert_eq!(client.get_alert_count(), 2);
        // remove -> count stays at 2
        client.remove_alert(&owner, &id2);
        assert_eq!(client.get_alert_count(), 2);

        // Cycle 3: register -> count goes to 3
        let id3 =
            client.register_alert(&owner, &target, &str(&env, "C"), &hash64(&env), &vec![&env]);
        assert_eq!(client.get_alert_count(), 3);
        // remove -> count stays at 3
        client.remove_alert(&owner, &id3);
        assert_eq!(client.get_alert_count(), 3);

        // Final verification: after 3 cycles the counter is 3, never reset to 0
        assert_eq!(client.get_alert_count(), 3);
        // No active alerts remain
        assert_eq!(client.get_active_alert_count(&owner), 0);
    }

    // 6. Edge case — get nonexistent alert returns None
    #[test]
    fn test_get_nonexistent_alert() {
        let (_env, client) = setup();
        assert!(client.get_alert(&999u64).is_none());
    }

    // 7. Edge case — get alerts for contract with no alerts returns empty vec
    #[test]
    fn test_get_alerts_for_contract_empty() {
        let (env, client) = setup();
        let querier = Address::generate(&env);
        let target = Address::generate(&env);
        let result = client.get_alerts_for_contract(&querier, &target);
        assert_eq!(result.len(), 0);
    }

    // Issue #68 — get_alerts_by_owner returns empty vec for address with no alerts
    #[test]
    fn test_get_alerts_by_owner_empty() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let querier = Address::generate(&env);
        assert_eq!(client.get_alerts_by_owner(&querier, &owner).len(), 0);
    }

    // 8. Index queries — get_alerts_for_contract and get_alerts_by_owner
    #[test]
    fn test_index_queries() {
        let (env, client) = setup();
        let querier = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(
            &owner,
            &target,
            &str(&env, "A1"),
            &hash64(&env),
            &vec![&env],
        );
        client.register_alert(
            &owner,
            &target,
            &str(&env, "A2"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.get_alerts_for_contract(&querier, &target).len(), 2);
        assert_eq!(client.get_alerts_by_owner(&querier, &owner).len(), 2);
    }

    // 9. get_alert_count reflects registered alerts (monotonic — does not decrease)
    #[test]
    fn test_get_alert_count() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        assert_eq!(client.get_alert_count(), 0u64);

        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        assert_eq!(client.get_alert_count(), 1u64);
    }

    // 10. Paginated queries work without watcher gating
    #[test]
    fn test_paginated_queries_no_gating() {
        let (env, client) = setup();
        let querier = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        for i in 0..5u32 {
            let label = String::from_str(&env, "alert");
            let _ = i; // suppress unused warning
            client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
        }

        let page = client.get_contract_alerts_paginated(&querier, &target, &0u32, &3u32);
        assert_eq!(page.len(), 3);

        let page2 = client.get_alerts_by_owner_paginated(&querier, &owner, &3u32, &10u32);
        assert_eq!(page2.len(), 2);
    }

    // ── Watcher-gating tests ──────────────────────────────────────────────

    // 11. No watcher registry configured — any querier can read
    #[test]
    fn test_no_watcher_registry_any_querier_can_read() {
        let (env, client) = setup();
        let stranger = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // No registry set — stranger can still query
        assert_eq!(client.get_alerts_for_contract(&stranger, &target).len(), 1);
    }

    // 12. Watcher registry configured — registered watcher can read
    #[test]
    #[cfg(feature = "testutils")]
    fn test_watcher_registry_registered_watcher_can_read() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        watcher_client.initialize(&admin);
        watcher_client.register_watcher(&admin, &watcher);

        // Point alert registry at the watcher registry
        alert_client.initialize(&admin);
        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        alert_client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Registered watcher can query
        let results = alert_client.get_alerts_for_contract(&watcher, &target);
        assert_eq!(results.len(), 1);
    }

    // 13. Watcher registry configured — unregistered address is rejected
    #[test]
    #[cfg(feature = "testutils")]
    fn test_watcher_registry_unregistered_address_rejected() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        let stranger = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        watcher_client.initialize(&admin);

        alert_client.initialize(&admin);
        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        alert_client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Stranger (not a watcher) is rejected
        assert_eq!(
            alert_client
                .try_get_alerts_for_contract(&stranger, &target)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );
    }

    // 14. Watcher registry configured — removed watcher loses access
    #[test]
    #[cfg(feature = "testutils")]
    fn test_watcher_registry_removed_watcher_loses_access() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        watcher_client.initialize(&admin);
        watcher_client.register_watcher(&admin, &watcher);

        alert_client.initialize(&admin);
        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        alert_client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Watcher can read before removal
        assert_eq!(
            alert_client
                .get_alerts_for_contract(&watcher, &target)
                .len(),
            1
        );

        // Remove the watcher
        watcher_client.remove_watcher(&admin, &watcher);

        // Now rejected
        assert_eq!(
            alert_client
                .try_get_alerts_for_contract(&watcher, &target)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );
    }

    // 15. get_watcher_registry returns None before configuration
    #[test]
    fn test_get_watcher_registry_none_before_set() {
        let (_env, client) = setup();
        assert!(client.get_watcher_registry().is_none());
        assert!(!client.is_watcher_gating_enabled());
    }

    // 16. set_watcher_registry persists and get_watcher_registry returns it
    #[test]
    #[cfg(feature = "testutils")]
    fn test_set_and_get_watcher_registry() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        alert_client.initialize(&admin);

        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        assert_eq!(
            alert_client.get_watcher_registry().unwrap(),
            watcher_contract_id
        );
        assert!(alert_client.is_watcher_gating_enabled());
    }

    // 16b. is_watcher_gating_enabled convenience getter
    #[test]
    #[cfg(feature = "testutils")]
    fn test_is_watcher_gating_enabled() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();
        assert!(!alert_client.is_watcher_gating_enabled());

        let admin = Address::generate(&env);
        alert_client.initialize(&admin);

        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        assert!(alert_client.is_watcher_gating_enabled());
    }

    // 17. Only admin can set watcher registry
    #[test]
    fn test_set_watcher_registry_non_admin_rejected() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        let fake_registry = Address::generate(&env);

        client.initialize(&admin);

        assert_eq!(
            client
                .try_set_watcher_registry(&attacker, &fake_registry)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 18. updated_at is strictly greater than created_at after update_alert
    //
    // The Soroban test environment starts with timestamp 0 and does not
    // advance automatically. We manually bump the ledger timestamp by 1
    // second between registration and update so that the contract's
    // `env.ledger().timestamp()` call inside `update_alert` returns a
    // value that is strictly greater than the one captured at registration.
    #[test]
    fn test_updated_at_strictly_greater_than_created_at() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Register at timestamp T (default = 0).
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Timestamp Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        let before = client.get_alert(&id).unwrap();
        assert_eq!(
            before.created_at, before.updated_at,
            "created_at and updated_at should be equal right after registration"
        );

        // Advance the ledger clock by 1 second so the update lands at T+1.
        env.ledger().with_mut(|li| {
            li.timestamp += 1;
        });

        client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true);

        let after = client.get_alert(&id).unwrap();
        assert!(
            after.updated_at > after.created_at,
            "updated_at ({}) must be strictly greater than created_at ({})",
            after.updated_at,
            after.created_at
        );
    }

    // 19. Register an alert with exactly 50 valid rule strings.
    //
    // This verifies that the contract handles the maximum allowed rule count
    // without hitting Soroban instruction limits. We alternate between the
    // two valid rule descriptors ("rule:transfer" and "rule:mint") to fill
    // all 50 slots, then confirm every entry is stored correctly.
    #[test]
    fn test_register_alert_with_50_rules_no_instruction_limit() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Build a vec of 50 valid rules, alternating between the two
        // accepted descriptors so the list is realistic.
        let mut rules: Vec<String> = vec![&env];
        for i in 0..50u32 {
            let rule = if i % 2 == 0 {
                str(&env, "rule:transfer")
            } else {
                str(&env, "rule:mint")
            };
            rules.push_back(rule);
        }

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Bulk Rules Alert"),
            &hash64(&env),
            &rules,
        );

        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.rules.len(), 50, "all 50 rules should be persisted");

        // Spot-check a few entries to confirm data integrity.
        assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
        assert_eq!(cfg.rules.get(1).unwrap(), str(&env, "rule:mint"));
        assert_eq!(cfg.rules.get(48).unwrap(), str(&env, "rule:transfer"));
        assert_eq!(cfg.rules.get(49).unwrap(), str(&env, "rule:mint"));
    }

    // ── Feature A: update_label ───────────────────────────────────────────────

    // 18. Happy path — update_label changes only the label
    #[test]
    fn test_update_label_changes_label() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Original"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        assert_eq!(
            client
                .try_update_label(&owner, &id, &str(&env, "Renamed"))
                .unwrap(),
            Ok(())
        );

        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.label, str(&env, "Renamed"));
        // rules and webhook_hash must be untouched
        assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
        assert_eq!(cfg.webhook_hash, hash64(&env));
        assert!(cfg.active);
    }

    // 19. update_label — unauthorized caller is rejected
    #[test]
    fn test_update_label_unauthorized() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(
            client
                .try_update_label(&attacker, &id, &str(&env, "Hacked"))
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 20. update_label — nonexistent alert returns AlertNotFound
    #[test]
    fn test_update_label_not_found() {
        let (env, client) = setup();
        let caller = Address::generate(&env);

        assert_eq!(
            client
                .try_update_label(&caller, &999u64, &str(&env, "X"))
                .unwrap_err()
                .unwrap(),
            ContractError::AlertNotFound
        );
    }

    // 21. update_label — label exceeding 128 bytes is rejected
    #[test]
    #[should_panic(expected = "Error(Contract, #7)")]
    fn test_update_label_too_long() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        client.update_label(&owner, &id, &str(&env, &"a".repeat(129)));
    }

    // 22. update_label — exactly 128 bytes is accepted
    #[test]
    fn test_update_label_max_length_accepted() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(
            client
                .try_update_label(&owner, &id, &str(&env, &"a".repeat(128)))
                .unwrap(),
            Ok(())
        );
    }

    // ── Feature B: get_active_alerts_for_contract ─────────────────────────────

    // 23. Happy path — only active alerts are returned
    #[test]
    fn test_get_active_alerts_for_contract_filters_inactive() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id1 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Active"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        let id2 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Inactive"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        // Deactivate the second alert
        client.update_alert(&owner, &id2, &vec![&env, str(&env, "rule:mint")], &false);

        let all = client.get_alerts_for_contract(&owner, &target);
        assert_eq!(all.len(), 2);

        let active = client.get_active_alerts_for_contract(&target);
        assert_eq!(active.len(), 1);
        assert_eq!(active.get(0).unwrap().label, str(&env, "Active"));
        let _ = id1;
    }

    // 24. get_active_alerts_for_contract — returns empty when all are inactive
    #[test]
    fn test_get_active_alerts_for_contract_all_inactive() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &false);

        let active = client.get_active_alerts_for_contract(&target);
        assert_eq!(active.len(), 0);
    }

    // 25. get_active_alerts_for_contract — returns empty for unknown contract
    #[test]
    fn test_get_active_alerts_for_contract_empty() {
        let (env, client) = setup();
        let target = Address::generate(&env);
        assert_eq!(client.get_active_alerts_for_contract(&target).len(), 0);
    }

    // 26. get_active_alerts_for_contract — all active alerts are returned
    #[test]
    fn test_get_active_alerts_for_contract_all_active() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(
            &owner,
            &target,
            &str(&env, "A1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        client.register_alert(
            &owner,
            &target,
            &str(&env, "A2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        let active = client.get_active_alerts_for_contract(&target);
        assert_eq!(active.len(), 2);
    }

    // 18. transfer_admin emits an ("admin", "transfer") event
    #[test]
    fn test_transfer_admin_emits_event() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        client.transfer_admin(&admin, &new_admin);

        // Verify at least one event was published during the transfer
        assert!(!env.events().all().is_empty());
    }

    // 19. old admin cannot act after transfer_admin
    #[test]
    fn test_old_admin_rejected_for_remove_alert_by_admin() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.transfer_admin(&admin, &new_admin);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        // old admin can no longer perform admin actions
        assert_eq!(
            client
                .try_remove_alert_by_admin(&admin, &id)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // ── get_alerts_modified_since ─────────────────────────────────────────────

    // 18. Returns all alerts when since == 0
    #[test]
    fn test_get_alerts_modified_since_zero_returns_all() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

        let results = client.get_alerts_modified_since(&0u64);
        assert_eq!(results.len(), 2);
    }

    // 19. Returns empty vec when no alerts exist
    #[test]
    fn test_get_alerts_modified_since_empty_registry() {
        let (_env, client) = setup();
        let results = client.get_alerts_modified_since(&0u64);
        assert_eq!(results.len(), 0);
    }

    // 20. Filters out alerts whose updated_at is before `since`
    #[test]
    fn test_get_alerts_modified_since_filters_old_alerts() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Register at ledger timestamp 0 (default in tests)
        let _id1 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Old"),
            &hash64(&env),
            &vec![&env],
        );

        // Advance the ledger timestamp so the next alert has a higher updated_at
        env.ledger().with_mut(|li| li.timestamp = 1000);

        let _id2 = client.register_alert(
            &owner,
            &target,
            &str(&env, "New"),
            &hash64(&env),
            &vec![&env],
        );

        // Query with since = 1000 — should only return the second alert
        let results = client.get_alerts_modified_since(&1000u64);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).unwrap().label, str(&env, "New"));
    }

    // 21. An updated alert appears in a subsequent incremental sync
    #[test]
    fn test_get_alerts_modified_since_includes_updated_alert() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Register both alerts at timestamp 0
        let id1 =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        let _id2 =
            client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

        // Advance time and update the first alert
        env.ledger().with_mut(|li| li.timestamp = 500);
        client.update_alert(&owner, &id1, &vec![&env], &false);

        // Incremental sync from timestamp 500 should return only the updated alert
        let results = client.get_alerts_modified_since(&500u64);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).unwrap().label, str(&env, "A"));
    }

    // 22. Removed alerts are not returned
    #[test]
    fn test_get_alerts_modified_since_excludes_removed_alerts() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id1 =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

        client.remove_alert(&owner, &id1);

        // Only the surviving alert should be returned
        let results = client.get_alerts_modified_since(&0u64);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).unwrap().label, str(&env, "B"));
    }

    // 23. since is exclusive of nothing — boundary value exactly equal is included
    #[test]
    fn test_get_alerts_modified_since_boundary_inclusive() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        env.ledger().with_mut(|li| li.timestamp = 42);
        client.register_alert(
            &owner,
            &target,
            &str(&env, "Boundary"),
            &hash64(&env),
            &vec![&env],
        );

        // since == updated_at should be inclusive
        let results = client.get_alerts_modified_since(&42u64);
        assert_eq!(results.len(), 1);

        // since == updated_at + 1 should exclude it
        let results_after = client.get_alerts_modified_since(&43u64);
        assert_eq!(results_after.len(), 0);
    }

    // ── Auth-failure tests (no mock_all_auths) ────────────────────────────────

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_register_alert_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_update_alert_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        // register with mocked auth first, then call update without auth
        env.mock_all_auths();
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "A"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );
        env.set_auths(&[]);
        client.update_alert(&owner, &id, &vec![&env], &false);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_update_webhook_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.update_webhook(&owner, &id, &hash64c(&env, 'b'));
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_remove_alert_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.remove_alert(&owner, &id);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_transfer_admin_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        env.set_auths(&[]);
        client.transfer_admin(&admin, &new_admin);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_set_per_owner_alert_limit_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.set_per_owner_alert_limit(&admin, &5u32);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_remove_alert_by_admin_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "A"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );
        env.set_auths(&[]);
        client.remove_alert_by_admin(&admin, &id);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_set_watcher_registry_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        let watcher_registry = Address::generate(&env);
        env.set_auths(&[]);
        client.set_watcher_registry(&admin, &watcher_registry);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_propose_webhook_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_confirm_webhook_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
        env.set_auths(&[]);
        client.confirm_webhook(&owner, &id);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_renew_alert_ttl_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.renew_alert_ttl(&owner, &id);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_update_label_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.update_label(&owner, &id, &str(&env, "New Label"));
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_deactivate_all_alerts_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        env.mock_all_auths();
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.deactivate_all_alerts(&owner);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_update_target_contract_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let new_target = Address::generate(&env);
        env.mock_all_auths();
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        env.set_auths(&[]);
        client.update_target_contract(&owner, &id, &new_target);
    }

    // ── Load & Scan-Cost Benchmarks (Issues #38, #39, #116) ───────────────────

    /// Load test quantifying the O(N) full-scan cost of `get_alerts_modified_since` (#38).
    /// Registers N alerts and benchmarks the CPU instruction cost of scanning the registry,
    /// establishing an upper bound budget regression guard.
    #[test]
    fn test_load_get_alerts_modified_since_instruction_cost() {
        const N: usize = 50;

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let hash = hash64(&env);
        let rules = vec![&env, str(&env, "rule:transfer")];

        for i in 0..N {
            let label = str(&env, "Alert");
            client.register_alert(&owner, &target, &label, &hash, &rules);
            // Stagger timestamps so every alert has a distinct updated_at
            if i % 5 == 0 {
                env.ledger().with_mut(|li| li.timestamp += 1);
            }
        }

        // Measure scan instruction cost across all N alerts
        let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
        let modified = client.get_alerts_modified_since(&0u64);
        let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
        let scan_cost = cpu_after.saturating_sub(cpu_before);

        assert_eq!(modified.len() as usize, N);
        // Assert an upper bound regression guard on the scan cost for N=50
        assert!(
            scan_cost < 15_000_000,
            "get_alerts_modified_since cost {scan_cost} exceeded upper bound 15M instructions"
        );
    }

    /// Load test quantifying the repeated rescan cost in `assert_per_owner_limit` (#39).
    /// Registers alerts with an active per-owner limit and benchmarks instruction growth,
    /// asserting an upper bound regression guard.
    #[test]
    fn test_load_assert_per_owner_limit_instruction_cost() {
        const LIMIT: u32 = 40;

        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_per_owner_alert_limit(&admin, &LIMIT);

        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let hash = hash64(&env);
        let rules = vec![&env];

        let mut first_reg_cost: u64 = 0;
        let mut last_reg_cost: u64 = 0;

        let total_cpu_before = env.cost_estimate().budget().cpu_instruction_cost();

        for i in 0..LIMIT {
            let label = str(&env, "LimitLoadAlert");
            let before = env.cost_estimate().budget().cpu_instruction_cost();
            client.register_alert(&owner, &target, &label, &hash, &rules);
            let after = env.cost_estimate().budget().cpu_instruction_cost();
            let cost = after.saturating_sub(before);

            if i == 0 {
                first_reg_cost = cost;
            } else if i == LIMIT - 1 {
                last_reg_cost = cost;
            }
        }

        let total_cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
        let total_registration_cost = total_cpu_after.saturating_sub(total_cpu_before);

        // Quantify that cost per registration includes the owner scan overhead
        assert!(
            first_reg_cost > 0 && last_reg_cost > 0,
            "Registration costs must be non-zero"
        );
        // Assert an upper bound regression guard on total batch registration cost with limit checks
        assert!(
            total_registration_cost < 50_000_000,
            "Total registration cost {total_registration_cost} exceeded upper bound 50M instructions"
        );
    }

    // #63 — ID monotonicity: each successive register_alert returns prev+1
    #[test]
    fn test_id_monotonicity() {
        const N: u64 = 10;

        let (env, client) = setup();

        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let mut prev_id: Option<u64> = None;
        for i in 0..N {
            let id = client.register_alert(
                &owner,
                &target,
                &str(&env, "alert"),
                &hash64(&env),
                &vec![&env],
            );
            if let Some(p) = prev_id {
                assert_eq!(
                    id,
                    p + 1,
                    "expected id {} but got {} at iteration {}",
                    p + 1,
                    id,
                    i
                );
            }
            prev_id = Some(id);
        }
    }

    // #33 — update_target_contract moves the alert to a new contract index
    #[test]
    fn test_update_target_contract() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let old_target = Address::generate(&env);
        let new_target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &old_target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        client.update_target_contract(&owner, &id, &new_target);

        // alert config reflects new target
        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.target_contract, new_target);

        // indexes updated correctly
        assert_eq!(client.get_alerts_for_contract(&owner, &old_target).len(), 0);
        assert_eq!(client.get_alerts_for_contract(&owner, &new_target).len(), 1);
    }

    // #33 — update_target_contract unauthorized
    #[test]
    fn test_update_target_contract_unauthorized() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let target = Address::generate(&env);
        let new_target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(
            client
                .try_update_target_contract(&attacker, &id, &new_target)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 18. update_alert after remove_alert returns AlertNotFound
    #[test]
    fn test_update_alert_after_remove_returns_not_found() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.try_remove_alert(&owner, &id).unwrap(), Ok(()));

        assert_eq!(
            client
                .try_update_alert(&owner, &id, &vec![&env], &false)
                .unwrap_err()
                .unwrap(),
            ContractError::AlertNotFound
        );
    }

    // 19. update_webhook after remove_alert returns AlertNotFound
    #[test]
    fn test_update_webhook_after_remove_returns_not_found() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.try_remove_alert(&owner, &id).unwrap(), Ok(()));

        assert_eq!(
            client
                .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
                .unwrap_err()
                .unwrap(),
            ContractError::AlertNotFound
        );
    }

    // 18. get_alert_active returns None for nonexistent ID
    #[test]
    fn test_get_alert_active_nonexistent() {
        let (_env, client) = setup();
        assert!(client.get_alert_active(&999u64).is_none());
    }

    // 19. get_alert_active returns true after registration
    #[test]
    fn test_get_alert_active_after_register() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.get_alert_active(&id), Some(true));
    }

    // 20. get_alert_active reflects update_alert changes
    #[test]
    fn test_get_alert_active_after_update() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.get_alert_active(&id), Some(true));

        client.update_alert(&owner, &id, &vec![&env], &false);
        assert_eq!(client.get_alert_active(&id), Some(false));

        client.update_alert(&owner, &id, &vec![&env], &true);
        assert_eq!(client.get_alert_active(&id), Some(true));
    }

    // 21. get_alert_active returns None after removal
    #[test]
    fn test_get_alert_active_after_remove() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.get_alert_active(&id), Some(true));
        client.remove_alert(&owner, &id);
        assert!(client.get_alert_active(&id).is_none());
    }

    // 22. deactivate_all_alerts returns 0 when owner has no alerts
    #[test]
    fn test_deactivate_all_alerts_empty() {
        let (env, client) = setup();
        let owner = Address::generate(&env);

        assert_eq!(client.deactivate_all_alerts(&owner), 0);
    }

    // 23. deactivate_all_alerts deactivates all alerts for the owner
    #[test]
    fn test_deactivate_all_alerts_multiple() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id1 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert 1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        let id2 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert 2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );
        let id3 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert 3"),
            &hash64c(&env, '3'),
            &vec![&env, str(&env, "rule:transfer")],
        );

        assert_eq!(client.get_alert_active(&id1), Some(true));
        assert_eq!(client.get_alert_active(&id2), Some(true));
        assert_eq!(client.get_alert_active(&id3), Some(true));

        let count = client.deactivate_all_alerts(&owner);
        assert_eq!(count, 3);

        assert_eq!(client.get_alert_active(&id1), Some(false));
        assert_eq!(client.get_alert_active(&id2), Some(false));
        assert_eq!(client.get_alert_active(&id3), Some(false));
    }

    // 24. deactivate_all_alerts only affects the calling owner's alerts
    #[test]
    fn test_deactivate_all_alerts_other_owner_unaffected() {
        let (env, client) = setup();
        let owner1 = Address::generate(&env);
        let owner2 = Address::generate(&env);
        let target = Address::generate(&env);

        let id1 = client.register_alert(
            &owner1,
            &target,
            &str(&env, "Owner1 Alert"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        let id2 = client.register_alert(
            &owner2,
            &target,
            &str(&env, "Owner2 Alert"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        let count = client.deactivate_all_alerts(&owner1);
        assert_eq!(count, 1);

        assert_eq!(client.get_alert_active(&id1), Some(false));
        assert_eq!(client.get_alert_active(&id2), Some(true));
    }

    // 25. deactivate_all_alerts skips removed alerts and deactivates remaining
    #[test]
    fn test_deactivate_all_alerts_after_removal() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id1 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert 1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        let id2 = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert 2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        client.remove_alert(&owner, &id1);

        let count = client.deactivate_all_alerts(&owner);
        assert_eq!(count, 1);

        // id1 is gone
        assert!(client.get_alert(&id1).is_none());
        // id2 is now inactive
        assert_eq!(client.get_alert_active(&id2), Some(false));
    }

    // 18. get_alerts_by_owner_paginated — basic pagination
    #[test]
    fn test_get_alerts_by_owner_paginated() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        for label in ["A", "B", "C", "D", "E"] {
            client.register_alert(
                &owner,
                &target,
                &str(&env, label),
                &hash64(&env),
                &vec![&env],
            );
        }

        // first page
        let page1 = client.get_alerts_by_owner_paginated(&owner, &owner, &0u32, &3u32);
        assert_eq!(page1.len(), 3);

        // second page
        let page2 = client.get_alerts_by_owner_paginated(&owner, &owner, &3u32, &3u32);
        assert_eq!(page2.len(), 2);

        // offset beyond length returns empty
        let empty = client.get_alerts_by_owner_paginated(&owner, &owner, &10u32, &3u32);
        assert_eq!(empty.len(), 0);
    }

    // 19. get_contract_alerts_paginated — basic pagination
    #[test]
    fn test_get_contract_alerts_paginated() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        for label in ["A", "B", "C", "D"] {
            client.register_alert(
                &owner,
                &target,
                &str(&env, label),
                &hash64(&env),
                &vec![&env],
            );
        }

        let page = client.get_contract_alerts_paginated(&owner, &target, &1u32, &2u32);
        assert_eq!(page.len(), 2);
    }

    // 18. get_admin panics with NotInitialized when contract is not initialized
    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_get_admin_not_initialized() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        client.get_admin();
    }

    // 19. Alert can be deactivated and reactivated via update_alert
    #[test]
    fn test_alert_deactivate_reactivate() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:mint")],
        );

        // deactivate
        assert_eq!(
            client
                .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false)
                .unwrap(),
            Ok(())
        );
        let cfg = client.get_alert(&id).unwrap();
        assert!(!cfg.active);

        // reactivate
        assert_eq!(
            client
                .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true)
                .unwrap(),
            Ok(())
        );
        let cfg = client.get_alert(&id).unwrap();
        assert!(cfg.active);
    }

    // 18. update_webhook advances updated_at beyond its value at registration
    #[test]
    fn test_update_webhook_updates_timestamp() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);

        let original_updated_at = client.get_alert(&id).unwrap().updated_at;
        env.ledger().set_timestamp(original_updated_at + 100);

        client
            .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap()
            .unwrap();

        let cfg = client.get_alert(&id).unwrap();
        assert!(cfg.updated_at > original_updated_at);
    }

    // 19. Multiple owners watching the same contract — indexes are isolated per owner
    #[test]
    fn test_multiple_owners_overlapping_target_contract() {
        let (env, client) = setup();
        let owner_a = Address::generate(&env);
        let owner_b = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(
            &owner_a,
            &target,
            &str(&env, "Alert-A"),
            &hash64c(&env, '5'),
            &vec![&env],
        );
        client.register_alert(
            &owner_b,
            &target,
            &str(&env, "Alert-B"),
            &hash64c(&env, '6'),
            &vec![&env],
        );

        assert_eq!(client.get_alerts_for_contract(&owner_a, &target).len(), 2);

        let alerts_a = client.get_alerts_by_owner(&owner_a, &owner_a);
        assert_eq!(alerts_a.len(), 1);
        assert_eq!(alerts_a.get(0).unwrap().owner, owner_a);

        let alerts_b = client.get_alerts_by_owner(&owner_b, &owner_b);
        assert_eq!(alerts_b.len(), 1);
        assert_eq!(alerts_b.get(0).unwrap().owner, owner_b);
    }

    // ── Feature B: configurable TTL via bump_alert ────────────────────────────

    // B-1. bump_alert succeeds for an existing alert
    #[test]
    fn test_bump_alert_succeeds() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        assert_eq!(client.try_bump_alert(&id, &17_280u32).unwrap(), Ok(()));
    }

    // B-2. bump_alert returns AlertNotFound for a non-existent ID
    #[test]
    fn test_bump_alert_not_found() {
        let (_env, client) = setup();
        assert_eq!(
            client
                .try_bump_alert(&999u64, &17_280u32)
                .unwrap_err()
                .unwrap(),
            ContractError::AlertNotFound
        );
    }

    // B-3. bump_alert clamps TTL above MAX_TTL to MAX_TTL
    #[test]
    fn test_bump_alert_clamps_to_max_ttl() {
        use soroban_sdk::testutils::Events as _;

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Request a TTL above the protocol maximum
        client.bump_alert(&id, &u32::MAX);

        // The emitted event should carry the clamped effective TTL
        let events = env.events().all();
        let bump_event = events.iter().find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap())
                    == soroban_sdk::symbol_short!("alert")
                && Symbol::from_val(&env, &topics.get(1).unwrap())
                    == soroban_sdk::symbol_short!("bump")
        });
        assert!(bump_event.is_some(), "expected an alert.bump event");

        let (_, _, data) = bump_event.unwrap();
        let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
        assert_eq!(emitted_id, id);
        assert_eq!(emitted_ttl, MAX_TTL, "TTL must be clamped to MAX_TTL");
    }

    // B-4. bump_alert with TTL below MAX_TTL uses the requested value exactly
    #[test]
    fn test_bump_alert_uses_requested_ttl_when_below_max() {
        use soroban_sdk::testutils::Events as _;

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        let requested_ttl: u32 = 120_960; // ~7 days, well below MAX_TTL
        client.bump_alert(&id, &requested_ttl);

        let events = env.events().all();
        let bump_event = events.iter().find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap())
                    == soroban_sdk::symbol_short!("alert")
                && Symbol::from_val(&env, &topics.get(1).unwrap())
                    == soroban_sdk::symbol_short!("bump")
        });
        assert!(bump_event.is_some());

        let (_, _, data) = bump_event.unwrap();
        let (_, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
        assert_eq!(emitted_ttl, requested_ttl);
    }

    // B-5. bump_alert emits the correct event shape (topic + data)
    #[test]
    fn test_bump_alert_event_shape() {
        use soroban_sdk::{symbol_short, testutils::Events as _};

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        let ttl: u32 = 17_280;
        client.bump_alert(&id, &ttl);

        let events = env.events().all();
        let bump_event = events
            .iter()
            .find(|(_, topics, _)| {
                topics.len() == 2
                    && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("alert")
                    && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("bump")
            })
            .expect("alert.bump event must be emitted");

        // Verify data shape: (id: u64, ttl: u32)
        let (_, _, data) = bump_event;
        let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
        assert_eq!(emitted_id, id);
        assert_eq!(emitted_ttl, ttl);
    }

    // B-6. bump_alert does not modify the alert's content
    #[test]
    fn test_bump_alert_does_not_modify_content() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Immutable"),
            &hash64c(&env, 'c'),
            &vec![&env, str(&env, "rule:transfer")],
        );

        let before = client.get_alert(&id).unwrap();
        client.bump_alert(&id, &17_280u32);
        let after = client.get_alert(&id).unwrap();

        // All fields must be identical after a bump
        assert_eq!(after.label, before.label);
        assert_eq!(after.webhook_hash, before.webhook_hash);
        assert_eq!(after.rules.len(), before.rules.len());
        assert_eq!(after.owner, before.owner);
        assert_eq!(after.target_contract, before.target_contract);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.active, before.active);
    }

    // B-7. DEFAULT_TTL and MAX_TTL constants have the expected values
    #[test]
    fn test_ttl_constants() {
        // MAX_TTL must be strictly greater than DEFAULT_TTL (checked at compile time)
        const _: () = assert!(MAX_TTL > DEFAULT_TTL);

        // DEFAULT_TTL ≈ 24 hours at 5 s/ledger
        assert_eq!(DEFAULT_TTL, 17_280);
        // MAX_TTL ≈ 31 days at 5 s/ledger
        assert_eq!(MAX_TTL, 535_680);
    }
}
