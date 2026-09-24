#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, vec,
    Address, Env, String, Vec,
    contract, contracterror, contractimpl, contractmeta, contracttype, panic_with_error,
    symbol_short, vec, Address, BytesN, Env, String, Vec,
};

contractmeta!(key = "Name", val = "AlertRegistry");
contractmeta!(key = "Version", val = "0.1.0");

// ── Storage keys ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "regression_tests.rs"]
mod regression_tests;
mod proptests;

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
    /// Stores the number of currently live (non-removed) alerts owned by a
    /// given address, maintained incrementally alongside [`DataKey::OwnerIndex`]
    /// so [`AlertRegistry::get_non_removed_alert_count`] never has to rescan
    /// the owner's full index.
    OwnerActiveCount(Address),
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
    /// Returned by `register_alert` when the global alert-count ceiling
    /// (set via `set_global_alert_limit`) has been reached.
    GlobalAlertLimitExceeded = 13,
    /// Returned by `register_alert` when the target contract is at the
    /// configured per-contract alert limit (set via
    /// `set_per_contract_alert_limit`).
    ContractAlertLimitExceeded = 14,
    /// Returned by `set_watcher_registry` when the given address does not
    /// respond to the `WatcherRegistry` interface (probed at configuration
    /// time), so gating would otherwise fail later inside
    /// `assert_watcher_if_configured` at query time.
    InvalidWatcherRegistry = 13,
    /// Returned when a state-mutating call is made while the contract is paused.
    Paused = 13,
    /// Returned by `validate_rules` when the same rule descriptor appears more
    /// than once in an alert's rule list.
    DuplicateRule = 15,
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

/// Input record for [`AlertRegistry::batch_register_alert`].
///
/// Mirrors the arguments of [`AlertRegistry::register_alert`] so a batch call
/// can register alerts for multiple owners/targets in one transaction.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AlertInput {
    /// Address that will own and control this alert.
    pub owner: Address,
    /// Contract address to watch.
    pub target_contract: Address,
    /// Human-readable name for the alert.
    pub label: String,
    /// SHA-256 hash of the destination webhook URL.
    pub webhook_hash: String,
    /// Rule identifiers that should trigger the alert.
    pub rules: Vec<String>,
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

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("init")),
            (admin,),
        );
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

    /// Replace this contract's WASM with `new_wasm_hash` (admin only).
    ///
    /// The new WASM must already be installed on-chain. Storage is untouched by
    /// the upgrade, so the new build **must** keep the existing [`DataKey`]
    /// layout and `NextId` counter — the host cannot verify this, and a build
    /// that changes them will read the existing entries as garbage. See
    /// `docs/upgrade-guide.md`.
    ///
    /// Requires the admin role to have been initialized: an uninitialized
    /// registry has no one authorized to upgrade it.
    ///
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not the admin.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        Ok(())
    }

    /// Get the current admin address.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&symbol_short!("ADMIN"))
            .ok_or(ContractError::NotInitialized)
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

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("limit")),
            (admin, limit),
        );
        Ok(())
    }

    /// Get the configured per-owner active alert limit, or `0` if none is set.
    pub fn get_per_owner_alert_limit(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("LIMIT"))
            .unwrap_or(0u32)
    }

    /// Set a per-contract active alert limit (admin only). A value of `0` means no limit.
    ///
    /// Symmetric to [`set_per_owner_alert_limit`]: that limit bounds how many
    /// alerts a single owner may register, while this one bounds how many
    /// alerts (contributed by any number of distinct owners) may target a
    /// single `target_contract`, closing the gap where a target contract
    /// could otherwise accumulate unbounded alerts.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Events
    /// Emits `(Symbol("admin"), Symbol("limit"))` with data `(Symbol("contract"), limit: u32)`.
    pub fn set_per_contract_alert_limit(
        env: Env,
        admin: Address,
        limit: u32,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&symbol_short!("CLIMIT"), &limit);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("limit")),
            (symbol_short!("contract"), limit),
        );
        Ok(())
    }

    /// Get the configured per-contract active alert limit, or `0` if none is set.
    pub fn get_per_contract_alert_limit(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("CLIMIT"))
            .unwrap_or(0u32)
    }

    /// Set a global ceiling on the total number of alerts ever registered
    /// (admin only). A value of `0` means no limit.
    ///
    /// This bounds the cost of registry-wide scans such as
    /// [`get_alerts_modified_since`], which iterate ID ranges derived from
    /// the total alert count: without a ceiling, an attacker could inflate
    /// that count via repeated [`register_alert`] calls to degrade the read
    /// path for every caller.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn set_global_alert_limit(
        env: Env,
        admin: Address,
        limit: u32,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&symbol_short!("GLIMIT"), &limit);
        Ok(())
    }

    /// Get the configured global alert-count ceiling, or `0` if none is set.
    pub fn get_global_alert_limit(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("GLIMIT"))
            .unwrap_or(0u32)
    }

    /// Configure the `WatcherRegistry` contract address used for optional
    /// watcher-gating on read queries (admin only).
    ///
    /// Once set, `get_alerts_for_contract`, `get_alerts_by_owner`, and their
    /// paginated variants will cross-call `WatcherRegistry::is_watcher_authorized`
    /// before returning data. Any address configured here — including a
    /// zero/default `Address` — is treated as a real registry and will be
    /// cross-called. Use [`AlertRegistry::clear_watcher_registry`] to disable
    /// gating.
    ///
    /// `watcher_registry` is probed with a read-only
    /// `is_watcher_authorized` call before being persisted, so a
    /// misconfigured address (not a contract, or a contract that doesn't
    /// implement the `WatcherRegistry` interface) is rejected here with a
    /// typed error instead of surfacing later as a panic inside
    /// `assert_watcher_if_configured` the next time a gated query runs.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// Returns [`ContractError::InvalidWatcherRegistry`] if `watcher_registry` does not respond
    /// to the `WatcherRegistry` interface.
    pub fn set_watcher_registry(
        env: Env,
        admin: Address,
        watcher_registry: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let probe = ExtWatcherClient::new(&env, &watcher_registry);
        if probe.try_is_watcher_authorized(&admin).is_err() {
            return Err(ContractError::InvalidWatcherRegistry);
        }

        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("WATCHREG"), &watcher_registry);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("watchreg")),
            (admin, watcher_registry),
        );
        Ok(())
    }

    /// Clear the configured `WatcherRegistry` contract address, disabling
    /// watcher-gating on the read queries (admin only).
    ///
    /// After this call, `get_alerts_for_contract`, `get_alerts_by_owner`, and
    /// their paginated variants no longer cross-call `WatcherRegistry` and
    /// behave as if gating had never been configured. Call
    /// [`AlertRegistry::set_watcher_registry`] again to re-enable gating.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn clear_watcher_registry(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage().instance().remove(&symbol_short!("WATCHREG"));
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
    /// Returns [`ContractError::ContractAlertLimitExceeded`] if the target contract is at the configured per-contract alert limit.
    /// Returns [`ContractError::GlobalAlertLimitExceeded`] if the registry is at the configured global alert-count ceiling.
    /// Returns [`ContractError::TooManyRules`] if `rules` exceeds the 50-rule maximum.
    /// Returns [`ContractError::InvalidRuleDescriptor`] if a rule is not a recognised descriptor.
    /// Returns [`ContractError::DuplicateRule`] if the same rule descriptor appears more than once.
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
        Self::assert_global_alert_limit(&env)?;
        Self::assert_per_owner_limit(&env, &owner)?;
        Self::assert_per_contract_limit(&env, &target_contract)?;

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
    /// Returns [`ContractError::DuplicateRule`] if the same rule descriptor appears more than once.
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("update")),
            (config_id, config.owner.clone(), active),
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("webhook")),
            (config_id, caller),
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("wh_conf")),
            (config_id, caller),
        );
        Ok(())
    }

    /// Abandon an in-progress webhook rotation, clearing the staged hash.
    ///
    /// Without this, the only way out of a staged rotation is to overwrite it
    /// with another proposal or confirm it — there is no clean way to back
    /// out. The live `webhook_hash` is never touched.
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
    /// Emits `(Symbol("alert"), Symbol("wh_cancel"))` with data `(id: u64, caller: Address)`.
    pub fn cancel_webhook_proposal(env: Env, caller: Address, config_id: u64) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        if config.pending_webhook_hash.is_none() {
            return Err(ContractError::NoPendingWebhook);
        }

        config.pending_webhook_hash = None;
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("wh_cancel")),
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
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("renew"))` with data `(id: u64, owner: Address)`.
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("renew")),
            (config_id, caller),
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
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("label"))` with data `(id: u64, caller: Address)`.
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("label")),
            (config_id, caller),
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

    /// Deactivate an alert without deleting its record (admin only).
    ///
    /// Unlike [`Self::remove_alert_by_admin`], the alert config and its
    /// indexes are left intact — only the `active` flag is cleared — so
    /// history is preserved for e.g. spam/abuse moderation.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("admin_off"))` with data `(id: u64, admin: Address)`.
    pub fn deactivate_alert_by_admin(
        env: Env,
        admin: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        config.active = false;
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);
        env.storage()
            .persistent()
            .set(&DataKey::AlertActive(config_id), &false);
        env.storage().persistent().extend_ttl(
            &DataKey::AlertActive(config_id),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("admin_off")),
            (config_id, admin),
        );
        Ok(())
    }

    /// Transfer ownership of an alert to a new address.
    ///
    /// Updates the [`AlertConfig::owner`] field and migrates the alert ID from
    /// the old owner's [`DataKey::OwnerIndex`] to the new owner's.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// current owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the current owner.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("transfer"))` with data
    /// `(id: u64, old_owner: Address, new_owner: Address)`.
    pub fn transfer_alert_ownership(
        env: Env,
        caller: Address,
        config_id: u64,
        new_owner: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        let old_owner = config.owner.clone();
        config.owner = new_owner.clone();
        config.updated_at = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::Alert(config_id), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Alert(config_id), DEFAULT_TTL, DEFAULT_TTL);

        Self::remove_from_owner_index(&env, &old_owner, config_id);
        Self::push_owner_index(&env, &new_owner, config_id)?;

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("transfer")),
            (config_id, old_owner, new_owner),
        );
        Ok(())
    }

    /// Register multiple alert configs in a single call.
    ///
    /// Each input is validated and authorized exactly as
    /// [`Self::register_alert`] would, and each successful registration emits
    /// the same `(Symbol("alert"), Symbol("register"))` event. If any input
    /// fails validation or authorization, the entire batch (including any
    /// alerts already registered earlier in the same call) is rolled back,
    /// since Soroban invocations are atomic.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from each input's `owner`.
    ///
    /// # Returns
    /// The new alerts' numeric IDs, in the same order as `inputs`.
    /// # Errors
    /// Returns the same errors as [`Self::register_alert`] for the failing item.
    pub fn batch_register_alert(
        env: Env,
        inputs: Vec<AlertInput>,
    ) -> Result<Vec<u64>, ContractError> {
        let mut ids: Vec<u64> = vec![&env];
        for i in 0..inputs.len() {
            let input = inputs.get(i).unwrap();
            let id = Self::register_alert(
                env.clone(),
                input.owner,
                input.target_contract,
                input.label,
                input.webhook_hash,
                input.rules,
            )?;
            ids.push_back(id);
        }
        Ok(ids)
    }

    /// Remove multiple alert configs owned by `caller` in a single call.
    ///
    /// Each ID is validated and authorized exactly as [`Self::remove_alert`]
    /// would, and each successful removal emits the same
    /// `(Symbol("alert"), Symbol("remove"))` event. If any ID does not exist
    /// or is not owned by `caller`, the entire batch is rolled back, since
    /// Soroban invocations are atomic.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if any `config_ids` entry does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if `caller` does not own every alert in `config_ids`.
    pub fn batch_remove_alert(
        env: Env,
        caller: Address,
        config_ids: Vec<u64>,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        for i in 0..config_ids.len() {
            let config_id = config_ids.get(i).unwrap();
            let config: AlertConfig = env
                .storage()
                .persistent()
                .get(&DataKey::Alert(config_id))
                .ok_or(ContractError::AlertNotFound)?;

            Self::assert_owner(&config, &caller)?;
            Self::remove_alert_record(&env, &config, config_id, &caller);
        }
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
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_active_alerts_for_contract(
        env: Env,
        querier: Address,
        target_contract: Address,
    ) -> Result<Vec<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        let ids = Self::contract_index(&env, &target_contract);
        Ok(Self::active_configs_for_ids(&env, &ids))
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

    /// Retrieve the raw list of alert IDs owned by a given address.
    ///
    /// Thin wrapper over the underlying `OwnerIndex` entry. Use this instead
    /// of [`Self::get_alerts_by_owner`] when only the IDs are needed (e.g. an
    /// existence check or a count) so callers don't pay the cost of
    /// deserializing every full [`AlertConfig`].
    ///
    /// Unlike [`Self::get_alerts_by_owner`], this is not subject to
    /// watcher-gating, since it exposes no alert content.
    ///
    /// Returns an empty vec if `owner` has no registered alerts.
    #[must_use]
    pub fn get_alert_ids_by_owner(env: Env, owner: Address) -> Vec<u64> {
        Self::owner_index(&env, &owner)
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
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alert(
        env: Env,
        querier: Address,
        config_id: u64,
    ) -> Result<Option<AlertConfig>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        Ok(env.storage().persistent().get(&DataKey::Alert(config_id)))
    }

    /// Read the `active` flag of an alert without deserializing the full config.
    ///
    /// Returns `None` if the alert does not exist or has expired.
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alert_active(
        env: Env,
        querier: Address,
        config_id: u64,
    ) -> Result<Option<bool>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::AlertActive(config_id)))
    }

    /// Read the owner of an alert without returning the full config.
    ///
    /// Thin wrapper over the stored [`AlertConfig`]: a separate owner-only
    /// storage key is not warranted because the owner never changes
    /// independently of the record (and `transfer_alert_ownership` rewrites
    /// the record anyway), so the cheap-read win would be nil. Callers
    /// checking only ownership no longer need to deserialize the config
    /// themselves.
    ///
    /// Returns `None` if the alert does not exist or has expired.
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alert_owner(
        env: Env,
        querier: Address,
        config_id: u64,
    ) -> Result<Option<Address>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        Ok(env
            .storage()
            .persistent()
            .get::<DataKey, AlertConfig>(&DataKey::Alert(config_id))
            .map(|cfg| cfg.owner))
    }

    /// Read the owner of an alert without returning the full config.
    ///
    /// Thin wrapper over the stored [`AlertConfig`]: a separate owner-only
    /// storage key is not warranted because the owner never changes
    /// independently of the record (`transfer_alert_ownership` rewrites the
    /// record anyway), so a second key would only add write cost. Callers
    /// checking only ownership no longer need to deserialize the config
    /// themselves.
    ///
    /// Returns `None` if the alert does not exist or has expired.
    ///
    /// If a `WatcherRegistry` is configured, `querier` must be a registered
    /// watcher or the call returns [`ContractError::NotAWatcher`].
    /// # Errors
    /// Returns [`ContractError::NotAWatcher`] if a watcher registry is configured
    /// and `querier` is not a registered watcher.
    pub fn get_alert_owner(
        env: Env,
        querier: Address,
        config_id: u64,
    ) -> Result<Option<Address>, ContractError> {
        Self::assert_watcher_if_configured(&env, &querier)?;
        Ok(env
            .storage()
            .persistent()
            .get::<DataKey, AlertConfig>(&DataKey::Alert(config_id))
            .map(|cfg| cfg.owner))
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
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("bulk_off"))` with data
    /// `(caller: Address, count: u32)` when at least one alert was deactivated.
    /// No event is emitted if `count` is `0`.
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
                    env.storage().persistent().extend_ttl(
                        &DataKey::ContractIndex(cfg.target_contract.clone()),
                        DEFAULT_TTL,
                        DEFAULT_TTL,
                    );
                    count += 1;
                }
            }
        }
        if count > 0 {
            env.storage().persistent().extend_ttl(
                &DataKey::OwnerIndex(caller.clone()),
                DEFAULT_TTL,
                DEFAULT_TTL,
            env.events().publish(
                (symbol_short!("alert"), symbol_short!("bulk_off")),
                (caller, count),
            );
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
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("retarget"))` with data
    /// `(id: u64, old_target: Address, new_target: Address)`.
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

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("retarget")),
            (config_id, old_target, new_target),
        );

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
    /// * `offset` - Number of IDs to skip from the start of the ID space.
    /// * `limit` - Maximum number of IDs to scan starting at `offset`.
    ///
    /// # Returns
    /// A `Vec<AlertConfig>` containing every live alert in the ID range
    /// `[offset, offset + limit)` (clamped to the current alert count) with
    /// `updated_at >= since`. Alerts that have been removed (and whose storage
    /// entry has therefore expired) are silently omitted.
    ///
    /// # Note
    /// The scan cost of a single call is bounded by `limit`, not by the total
    /// size of the registry, so callers should page through with a bounded
    /// `limit` (see [`get_global_alert_limit`] for an admin-settable ceiling
    /// on total registry size) rather than requesting the whole ID space in
    /// one call. Callers that need every alert should page repeatedly,
    /// advancing `offset` by `limit` each call until fewer than `limit`
    /// results are returned.
    #[must_use]
    pub fn get_alerts_modified_since(env: Env, since: u64, offset: u32, limit: u32) -> Vec<AlertConfig> {
        let total: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u64);

        let range_start = u64::from(offset).min(total);
        let range_end = u64::from(offset)
            .saturating_add(u64::from(limit))
            .min(total);

        let mut out: Vec<AlertConfig> = vec![&env];
        for id in range_start..range_end {
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
    /// decremented when alerts are removed. Use [`get_non_removed_alert_count`]
    /// if you need the number of currently live (non-removed) alerts for a
    /// given owner, or [`get_active_alert_count`] for the number that are
    /// still active.
    #[must_use]
    pub fn get_alert_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u64)
    }

    /// Get the number of currently active alerts owned by `owner`.
    ///
    /// Unlike [`get_alert_count`], this reflects removals and only counts
    /// alerts with `active == true` — deactivated-but-not-removed alerts are
    /// excluded, so the result matches the `active` flag of the alerts
    /// returned by [`Self::get_alerts_by_owner`].
    ///
    /// Scans the owner's [`DataKey::OwnerIndex`] and reads the cheap
    /// [`DataKey::AlertActive`] flag for each entry, so it reflects both
    /// removals and deactivations. If you only need the number of live
    /// (non-removed) alerts regardless of the `active` flag, use
    /// [`Self::get_non_removed_alert_count`], which is an O(1) lookup.
    #[must_use]
    pub fn get_active_alert_count(env: Env, owner: Address) -> u32 {
        let ids = Self::owner_index(&env, &owner);
        let mut count: u32 = 0;
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if env
                .storage()
                .persistent()
                .get::<DataKey, bool>(&DataKey::AlertActive(id))
                == Some(true)
            {
                count += 1;
            }
        }
        count
    }

    /// Get the number of currently live (non-removed) alerts owned by `owner`.
    ///
    /// Unlike [`Self::get_active_alert_count`], this does **not** filter by
    /// the `active` flag: deactivated-but-not-removed alerts still count.
    ///
    /// Backed by a running counter maintained incrementally by
    /// [`Self::push_owner_index`]/[`Self::remove_from_owner_index`], so this
    /// is an O(1) lookup regardless of how many alerts `owner` has ever
    /// registered — it never rescans the owner's index.
    #[must_use]
    pub fn get_non_removed_alert_count(env: Env, owner: Address) -> u32 {
        Self::owner_active_count(&env, &owner)
    }

    /// Get the number of currently active (non-removed) alerts targeting `target_contract`,
    /// aggregated across every contributing owner.
    ///
    /// Symmetric to [`get_active_alert_count`], but keyed by target contract
    /// instead of owner.
    /// # Panics
    /// Panics if the contract's stored state is malformed or missing.
    pub fn get_active_contract_alert_count(env: Env, target_contract: Address) -> u32 {
        let ids = Self::contract_index(&env, &target_contract);
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
        if limit > 0 && Self::get_non_removed_alert_count(env.clone(), owner.clone()) >= limit {
            return Err(ContractError::OwnerAlertLimitExceeded);
        }
        Ok(())
    }

    /// Reject registration once the number of currently active alerts
    /// targeting `target_contract` (across all contributing owners) reaches
    /// the configured per-contract limit. A limit of `0` means no limit.
    fn assert_per_contract_limit(env: &Env, target_contract: &Address) -> Result<(), ContractError> {
        let limit = Self::get_per_contract_alert_limit(env.clone());
        if limit > 0
            && Self::get_active_contract_alert_count(env.clone(), target_contract.clone()) >= limit
        {
            return Err(ContractError::ContractAlertLimitExceeded);
        }
        Ok(())
    }

    /// Reject registration once the total number of alerts ever registered
    /// (the monotonic [`NextId`](DataKey::NextId) counter) reaches the
    /// configured global ceiling. A limit of `0` means no ceiling.
    fn assert_global_alert_limit(env: &Env) -> Result<(), ContractError> {
        let limit = Self::get_global_alert_limit(env.clone());
        if limit > 0 && Self::get_alert_count(env.clone()) >= u64::from(limit) {
            return Err(ContractError::GlobalAlertLimitExceeded);
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

    /// Read the running per-owner live-alert counter, or `0` if unset.
    fn owner_active_count(env: &Env, owner: &Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerActiveCount(owner.clone()))
            .unwrap_or(0u32)
    }

    /// Persist the running per-owner live-alert counter with a refreshed TTL.
    fn set_owner_active_count(env: &Env, owner: &Address, count: u32) {
        env.storage()
            .persistent()
            .set(&DataKey::OwnerActiveCount(owner.clone()), &count);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerActiveCount(owner.clone()),
            DEFAULT_TTL,
            DEFAULT_TTL,
        );
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
        let count = Self::owner_active_count(env, owner);
        Self::set_owner_active_count(env, owner, count + 1);
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
        let mut removed = false;
        for i in 0..ids.len() {
            let v = ids.get(i).unwrap();
            if v == id {
                removed = true;
            } else {
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
        if removed {
            let count = Self::owner_active_count(env, owner);
            Self::set_owner_active_count(env, owner, count.saturating_sub(1));
        }
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

impl AlertRegistry {
    /// Validates a single rule descriptor string.
    ///
    /// Accepts only `"rule:transfer"` and `"rule:mint"`.
    /// Returns [`ContractError::InvalidRuleDescriptor`] on any other string.
    ///
    /// Exposed for testing, integration, and fuzz testing.
    /// # Errors
    /// Returns [`ContractError::InvalidRuleDescriptor`] if `rule` is not recognized.
    pub fn validate_rule(env: &Env, rule: &String) -> Result<(), ContractError> {
        let transfer = String::from_str(env, "rule:transfer");
        let mint = String::from_str(env, "rule:mint");
        if *rule != transfer && *rule != mint {
            return Err(ContractError::InvalidRuleDescriptor);
        }
        Ok(())
    }

    /// Validates a vector of rule descriptors.
    ///
    /// Ensures at most 50 rules are supplied, each rule matches a recognized
    /// prefix, and no descriptor appears more than once.
    /// Exposed for testing, integration, and fuzz testing.
    /// # Errors
    /// Returns [`ContractError::TooManyRules`] if rules length exceeds 50.
    /// Returns [`ContractError::InvalidRuleDescriptor`] if any rule descriptor is invalid.
    /// Returns [`ContractError::DuplicateRule`] if the same descriptor appears more than once.
    /// # Panics
    /// Panics if indexing into `rules` fails unexpectedly.
    pub fn validate_rules(env: &Env, rules: &Vec<String>) -> Result<(), ContractError> {
        if rules.len() > 50 {
            return Err(ContractError::TooManyRules);
        }
        // With only two recognized descriptors, each may appear at most once.
        let transfer = String::from_str(env, "rule:transfer");
        let mint = String::from_str(env, "rule:mint");
        let mut saw_transfer = false;
        let mut saw_mint = false;
        for i in 0..rules.len() {
            let rule = rules.get(i).unwrap();
            Self::validate_rule(env, &rule)?;
            if rule == transfer {
                if saw_transfer {
                    return Err(ContractError::DuplicateRule);
                }
                saw_transfer = true;
            } else if rule == mint {
                if saw_mint {
                    return Err(ContractError::DuplicateRule);
                }
                saw_mint = true;
            }
        }
        Ok(())
    }
}
