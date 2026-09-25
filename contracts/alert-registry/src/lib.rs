//! `AlertRegistry` — Soroban contract that stores alert configurations on-chain,
//! keyed by the contract address they watch.
//!
//! See `docs/alert-registry.md` for the function reference.
#![no_std]
// Doc coverage is enforced by scripts/check-docs.sh in CI rather than by
// `#![warn(missing_docs)]` here: Soroban's contract macros generate
// undocumented public items, which clippy's `-D warnings` would turn into
// errors. Intra-doc links inside `#[contractimpl]` must use the full type path
// (not `Self::`) because the macro copies method docs into generated modules.
#![warn(rustdoc::broken_intra_doc_links)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, vec,
    Address, Env, String, Vec,
    contract, contracterror, contractimpl, contractmeta, contracttype, panic_with_error,
    symbol_short, vec, Address, BytesN, Env, String, Vec,
};

contractmeta!(key = "Name", val = "AlertRegistry");
contractmeta!(key = "Version", val = "0.2.0");

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

/// Upper bound on the TTL, in ledgers, that [`AlertRegistry::bump_alert`]
/// applies.
///
/// Callers should request at most this value. The current build caps larger
/// requests at `MAX_TTL` and reports the TTL it actually applied in the
/// `alert.bump` event, but whether over-cap requests should be capped or
/// rejected is an open question (#28), so do not rely on the capping.
///
/// Approximately 31 days at the nominal 5-second ledger close time.
pub const MAX_TTL: u32 = 535_680;

/// Number of ledgers a proposed alert ownership transfer stays open for the
/// recipient to accept (approximately 7 days at the nominal 5-second ledger
/// close time). After that it can only be cancelled or replaced.
pub const ALERT_TRANSFER_EXPIRY_LEDGERS: u32 = 120_960;
/// Threshold, in ledgers, below which the contract's instance entry is
/// extended by [`AlertRegistry::bump_instance_ttl`] and by every write to
/// instance storage. Approximately 24 hours at the nominal 5-second ledger
/// close time. Mirrors `WatcherRegistry`.
pub const INSTANCE_BUMP_THRESHOLD: u32 = 17_280;

/// TTL, in ledgers, the instance entry is extended to. Approximately 31 days,
/// the protocol maximum. See `docs/ttl.md`.
pub const INSTANCE_BUMP_AMOUNT: u32 = 535_680;
/// Maximum number of IDs scanned by a single paginated query.
pub const MAX_PAGE_SIZE: u32 = 100;

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
    ///
    /// "Live" means not removed: deactivated alerts still count. Use
    /// [`AlertRegistry::get_active_alert_count`] for the `active`-filtered
    /// number. Formerly `OwnerActiveCount`; entries written under that key are
    /// migrated on first read (see `AlertRegistry::owner_live_count`).
    OwnerLiveCount(Address),
    /// Stores the list of alert IDs watching a given contract address.
    ContractIndex(Address),
    /// Monotonic counter used to generate unique alert IDs.
    NextId,
    /// Stores the [`PendingAlertTransfer`] proposed for an alert, until it is
    /// accepted, rejected, cancelled, or the alert is removed or retargeted.
    PendingTransfer(u64),
    /// Present (`true`) while an admin has suspended the alert with
    /// [`AlertRegistry::deactivate_alert_by_admin`]. The owner cannot
    /// reactivate a suspended alert until an admin calls
    /// [`AlertRegistry::unlock_alert_by_admin`]. A separate key rather than an
    /// [`AlertConfig`] field, so stored configs keep their encoding.
    AdminSuspended(u64),
    /// Stores the `Address` proposed as the next admin by
    /// [`AlertRegistry::propose_admin_transfer`], pending its own acceptance
    /// via [`AlertRegistry::accept_admin_transfer`].
    PendingAdminTransfer,
}

/// Keys of the entries in the contract's **instance** storage.
///
/// Each constant is the exact `symbol_short!` the contract has always used, so
/// the on-chain encoding is unchanged and no migration is needed; routing every
/// access through these names turns a mistyped key into a compile error.
/// They are deliberately not [`DataKey`] variants: a variant is encoded as
/// `[Symbol("Name"), ...]`, not as the bare symbol, so it would orphan the
/// values already stored under these keys.
pub mod instance_key {
    use soroban_sdk::{symbol_short, Symbol};

    /// `Address` of the admin (see [`crate::AlertRegistry::get_admin`]).
    pub const ADMIN: Symbol = symbol_short!("ADMIN");
    /// `u64` monotonic counter used to generate alert IDs; also the value of
    /// [`crate::AlertRegistry::get_alert_count`].
    pub const NEXT_ID: Symbol = symbol_short!("NEXT_ID");
    /// `bool` circuit-breaker flag set by `pause` / `unpause`.
    pub const PAUSED: Symbol = symbol_short!("PAUSED");
    /// `u32` per-owner alert limit (`0` = unlimited).
    pub const LIMIT: Symbol = symbol_short!("LIMIT");
    /// `u32` per-contract alert limit (`0` = unlimited).
    pub const CLIMIT: Symbol = symbol_short!("CLIMIT");
    /// `u32` ceiling on the total number of alerts ever registered (`0` = none).
    pub const GLIMIT: Symbol = symbol_short!("GLIMIT");
    /// `u64` number of currently live alert records.
    pub const LIVE: Symbol = symbol_short!("LIVE");
    /// `Address` of the optional `WatcherRegistry` used for read gating.
    pub const WATCHREG: Symbol = symbol_short!("WATCHREG");
}

/// Errors returned by `AlertRegistry` entry points.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// The caller is not the alert owner or the admin required for this call.
    Unauthorized = 1,
    /// No alert exists with the given ID (never registered, removed, or expired).
    AlertNotFound = 2,
    /// `initialize` was called on an already initialized contract.
    AlreadyInitialized = 3,
    /// An admin-only call was made before `initialize` set an admin.
    NotInitialized = 4,
    /// Returned when a watcher registry is configured and the querying address
    /// is not a registered watcher.
    NotAWatcher = 5,
    /// No longer returned. Webhook hashes are now typed `BytesN<32>`, so a
    /// wrong-length hash cannot be constructed (#214). The variant is kept so
    /// its discriminant is never reused for a different error.
    InvalidWebhookHash = 6,
    /// The label exceeds 128 bytes.
    LabelTooLong = 7,
    /// The rule list exceeds the 50-rule maximum.
    TooManyRules = 8,
    /// A rule is not a recognised rule descriptor.
    InvalidRuleDescriptor = 9,
    /// The owner is already at the per-owner alert limit
    /// (set via `set_per_owner_alert_limit`).
    OwnerAlertLimitExceeded = 10,
    /// The alert ID is already present in the owner or contract index
    /// (an internal invariant violation).
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
    /// Returned by `accept_alert_transfer`, `reject_alert_transfer` and
    /// `cancel_alert_transfer` when the alert has no pending transfer.
    NoPendingTransfer = 18,
    /// Returned by `accept_alert_transfer` when the pending transfer is past
    /// its expiry ledger.
    TransferExpired = 19,
    /// Returned by `propose_alert_transfer` when `new_owner` is already the owner.
    InvalidTransferRecipient = 20,
    /// Returned by `update_alert` when the owner tries to reactivate an alert
    /// that an admin suspended with `deactivate_alert_by_admin`.
    AlertSuspended = 21,
    /// Returned by `accept_admin_transfer` or `cancel_admin_transfer` when no
    /// admin transfer is currently pending, or when the accepting address does
    /// not match the proposed address.
    NoPendingTransfer = 16,
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
    /// SHA-256 hash of the webhook URL (the raw URL is never stored on-chain),
    /// as its 32 raw digest bytes.
    pub webhook_hash: BytesN<32>,
    /// Staged replacement for `webhook_hash` during a two-phase rotation.
    ///
    /// Set by [`AlertRegistry::propose_webhook`] and promoted to `webhook_hash`
    /// by [`AlertRegistry::confirm_webhook`]. `None` when no rotation is in
    /// progress. Staging the change means a misconfigured endpoint never
    /// silently replaces a working one.
    pub pending_webhook_hash: Option<BytesN<32>>,
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
    /// Monotonic ledger sequence number of the most recent update.
    pub updated_ledger: u32,
    /// Whether the alert is currently active.
    pub active: bool,
}

/// An ownership transfer proposed by an alert's owner and awaiting the
/// recipient's acceptance. Stored under [`DataKey::PendingTransfer`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingAlertTransfer {
    /// Address that must accept the transfer.
    pub new_owner: Address,
    /// Last ledger sequence at which the transfer can still be accepted.
    pub expires_at_ledger: u32,
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
    /// SHA-256 hash of the destination webhook URL, as its 32 raw digest bytes.
    pub webhook_hash: BytesN<32>,
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
/// [`AlertRegistry::set_watcher_registry`], every read that returns alert
/// content performs a cross-contract call to verify that its `querier`
/// argument is a registered watcher before returning data:
///
/// - [`AlertRegistry::get_alert`]
/// - [`AlertRegistry::get_alert_active`]
/// - [`AlertRegistry::get_alert_owner`]
/// - [`AlertRegistry::get_alerts_for_contract`]
/// - [`AlertRegistry::get_active_alerts_for_contract`]
/// - [`AlertRegistry::get_alerts_by_owner`]
/// - [`AlertRegistry::get_contract_alerts_paginated`]
/// - [`AlertRegistry::get_alerts_by_owner_paginated`]
///
/// Callers that are not registered watchers receive
/// [`ContractError::NotAWatcher`]. Reads that expose only IDs or counts
/// (for example [`AlertRegistry::get_alert_ids_by_owner`]) are not gated.
///
/// If no watcher registry is configured the gating is skipped and the
/// functions behave as before.
///
/// # Storage and TTL
/// All persistent entries are extended by [`DEFAULT_TTL`] ledgers (~24 hours) on every
/// write. Callers can extend any alert up to [`MAX_TTL`] ledgers (~31 days) via
/// [`AlertRegistry::bump_alert`]. See `docs/ttl.md` for full details.
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

    /// Atomic constructor called during contract deployment to initialize the bootstrap admin.
    ///
    /// Running atomically with deployment prevents front-running the initialization window.
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&instance_key::ADMIN, &admin);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("init")),
            (admin,),
        );
    }

    /// Initialize the optional admin role for the registry. Can only be called once.
    ///
    /// Kept for backwards compatibility. If the contract was initialized
    /// via [`Self::__constructor`] during deployment, calling this again returns
    /// [`ContractError::AlreadyInitialized`].
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`. This prevents a
    /// front-running attack where an arbitrary account claims the admin role
    /// during the window between deployment and legitimate initialization.
    /// # Errors
    /// Returns [`ContractError::AlreadyInitialized`] if the contract has already been initialized.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&symbol_short!("ADMIN")) {
        if env.storage().instance().has(&instance_key::ADMIN) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&instance_key::ADMIN, &admin);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("init")),
            (admin,),
        );
        Ok(())
    }

    /// Transfer the admin role to a new address (admin only).
    ///
    /// # Deprecation
    /// This function transfers admin in a single call without the new admin's
    /// signature. A typo'd `new_admin` permanently locks the contract. Use the
    /// two-step [`Self::propose_admin_transfer`] /
    /// [`Self::accept_admin_transfer`] flow instead, which requires the
    /// incoming admin to prove key ownership before the transfer takes effect.
    ///
    /// Kept for backwards compatibility; scheduled for removal in v0.3.0.
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
            .set(&instance_key::ADMIN, &new_admin);
        Self::extend_instance_ttl(&env);

        // Admin handover is security-relevant: emit it so off-chain watchers
        // can react to a change of control.
        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transfer")),
            (admin, new_admin),
        );
        Ok(())
    }

    /// Propose transferring the admin role to `new_admin` (current admin only).
    ///
    /// The transfer does not take effect until `new_admin` calls
    /// [`Self::accept_admin_transfer`] with their own signature, proving key
    /// ownership before control passes. This prevents a typo'd or unowned
    /// address from permanently locking upgrade, pause, and every limit setter.
    ///
    /// Calling this again while a proposal is already pending overwrites the
    /// previous proposal with the new `new_admin`. To withdraw a proposal
    /// entirely, call [`Self::cancel_admin_transfer`].
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be the
    /// current admin.
    ///
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if `admin` is not the current admin.
    ///
    /// # Events
    /// Emits `(Symbol("admin"), Symbol("propose"))` with data
    /// `(admin: Address, new_admin: Address)`.
    pub fn propose_admin_transfer(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::assert_not_paused(&env)?;

        env.storage()
            .instance()
            .set(&DataKey::PendingAdminTransfer, &new_admin);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("propose")),
            (admin, new_admin),
        );

        Ok(())
    }

    /// Accept a pending admin transfer proposed via [`Self::propose_admin_transfer`].
    ///
    /// Requires `new_admin`'s own signature, proving key control before
    /// `ADMIN` storage is updated. Emits `(Symbol("admin"), Symbol("transfer"))`
    /// with data `(new_admin: Address)` once the handover completes.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `new_admin`.
    ///
    /// # Errors
    /// Returns [`ContractError::NoPendingTransfer`] if no transfer is pending or
    /// the pending proposal names a different address than `new_admin`.
    ///
    /// # Events
    /// Emits `(Symbol("admin"), Symbol("transfer"))` with data `(new_admin: Address)`.
    pub fn accept_admin_transfer(env: Env, new_admin: Address) -> Result<(), ContractError> {
        new_admin.require_auth();

        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdminTransfer)
            .ok_or(ContractError::NoPendingTransfer)?;

        if pending != new_admin {
            return Err(ContractError::NoPendingTransfer);
        }

        env.storage()
            .instance()
            .set(&symbol_short!("ADMIN"), &new_admin);
        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminTransfer);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transfer")),
            new_admin,
        );

        Ok(())
    }

    /// Cancel a pending admin transfer proposed via [`Self::propose_admin_transfer`]
    /// (current admin only).
    ///
    /// The pending proposal is cleared; the current admin is unchanged.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`, who must be the
    /// current admin.
    ///
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if `admin` is not the current admin.
    /// Returns [`ContractError::NoPendingTransfer`] if no transfer is currently pending.
    ///
    /// # Events
    /// Emits `(Symbol("admin"), Symbol("cancel"))` with data `(admin: Address)`.
    pub fn cancel_admin_transfer(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        if !env
            .storage()
            .instance()
            .has(&DataKey::PendingAdminTransfer)
        {
            return Err(ContractError::NoPendingTransfer);
        }

        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminTransfer);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("cancel")),
            admin,
        );

        Ok(())
    }

    /// Replace this contract's WASM with `new_wasm_hash` (admin only).
    ///
    /// The new WASM must already be installed on-chain. Storage is untouched by
    /// the upgrade, so the new build **must** keep the existing [`DataKey`]
    /// layout and [`instance_key`] entries (including the `NEXT_ID` counter)
    /// — the host cannot verify this, and a build that changes them will read
    /// the existing entries as garbage. See
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

    /// Extend the TTL of the contract's instance entry, which holds the admin,
    /// the alert ID counter, the alert limits, the pause flag and the watcher
    /// registry address. If it is archived, every alert becomes unreachable
    /// until the entry is restored.
    ///
    /// Callable by anyone and requires no auth: it only refreshes the entry's
    /// lifetime and never reads or changes registry state. Every write to
    /// instance storage already extends it, so this is for deployments that
    /// go quiet; have a keeper call it periodically (see `docs/ttl.md`).
    pub fn bump_instance_ttl(env: Env) {
        Self::extend_instance_ttl(&env);
    }

    /// Get the current admin address.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&instance_key::ADMIN)
            .ok_or(ContractError::NotInitialized)
    }

    /// Pause the contract, rejecting all state-mutating calls until [`AlertRegistry::unpause`] is called.
    ///
    /// Intended as an emergency circuit-breaker if an admin key is suspected
    /// compromised — mutations can be frozen while the incident is investigated.
    ///
    /// Every state-mutating entry point returns [`ContractError::Paused`] while
    /// paused, with these deliberate exemptions:
    /// - `pause` / `unpause`, so the circuit-breaker can be operated;
    /// - `initialize`, the one-time admin bootstrap;
    /// - `upgrade`, so a hot-fix can be deployed during an incident.
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage().instance().set(&instance_key::PAUSED, &true);
        Self::extend_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("admin"), symbol_short!("pause")), admin);
        Ok(())
    }

    /// Resume normal operation after a [`AlertRegistry::pause`].
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        env.storage().instance().set(&instance_key::PAUSED, &false);
        Self::extend_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("admin"), symbol_short!("unpause")), admin);
        Ok(())
    }

    /// Return `true` if the contract is currently paused.
    #[must_use]
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&instance_key::PAUSED)
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
        env.storage().instance().set(&instance_key::LIMIT, &limit);
        Self::extend_instance_ttl(&env);

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
            .get(&instance_key::LIMIT)
            .unwrap_or(0u32)
    }

    /// Set a per-contract active alert limit (admin only). A value of `0` means no limit.
    ///
    /// Symmetric to [`AlertRegistry::set_per_owner_alert_limit`]: that limit bounds how many
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
        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("CLIMIT"), &limit);
        env.storage().instance().set(&instance_key::CLIMIT, &limit);
        Self::extend_instance_ttl(&env);

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
            .get(&instance_key::CLIMIT)
            .unwrap_or(0u32)
    }

    /// Set a global ceiling on the total number of alerts ever registered
    /// (admin only). A value of `0` means no limit.
    ///
    /// This bounds the cost of registry-wide scans such as
    /// [`AlertRegistry::get_alerts_modified_since`], which iterate ID ranges derived from
    /// the total alert count: without a ceiling, an attacker could inflate
    /// that count via repeated [`AlertRegistry::register_alert`] calls to degrade the read
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
        Self::assert_not_paused(&env)?;
        env.storage()
            .instance()
            .set(&symbol_short!("GLIMIT"), &limit);
        env.storage().instance().set(&instance_key::GLIMIT, &limit);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Get the configured global alert-count ceiling, or `0` if none is set.
    pub fn get_global_alert_limit(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&instance_key::GLIMIT)
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
        Self::assert_not_paused(&env)?;

        let probe = ExtWatcherClient::new(&env, &watcher_registry);
        if probe.try_is_watcher_authorized(&admin).is_err() {
            return Err(ContractError::InvalidWatcherRegistry);
        }

        env.storage()
            .instance()
            .set(&instance_key::WATCHREG, &watcher_registry);
        Self::extend_instance_ttl(&env);

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
        Self::assert_not_paused(&env)?;
        env.storage().instance().remove(&symbol_short!("WATCHREG"));
        env.storage().instance().remove(&instance_key::WATCHREG);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Return the configured `WatcherRegistry` contract address, or `None` if
    /// watcher-gating has not been enabled.
    pub fn get_watcher_registry(env: Env) -> Option<Address> {
        env.storage().instance().get(&instance_key::WATCHREG)
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
    /// * `webhook_hash` - SHA-256 hash of the destination webhook URL, as its 32 raw digest bytes.
    /// * `rules` - Rule identifiers that should trigger the alert.
    ///
    /// # Returns
    /// The new alert's numeric ID.
    /// # Errors
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
        webhook_hash: BytesN<32>,
        rules: Vec<String>,
    ) -> Result<u64, ContractError> {
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
            updated_ledger: env.ledger().sequence(),
            active: true,
        };

        Self::push_owner_index(&env, &owner, id)?;
        Self::push_contract_index(&env, &target_contract, id)?;
        Self::persist_alert(&env, id, &config);
        Self::set_live_alert_count(
            &env,
            Self::get_live_alert_count(&env).saturating_add(1),
        );

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
    /// Returns [`ContractError::AlertSuspended`] if `active` is `true` while an admin has
    /// suspended the alert (see [`AlertRegistry::deactivate_alert_by_admin`]).
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
        if active
            && env
                .storage()
                .persistent()
                .has(&DataKey::AdminSuspended(config_id))
        {
            return Err(ContractError::AlertSuspended);
        }

        config.rules = rules;
        config.active = active;
        config.updated_at = env.ledger().timestamp();
        config.updated_ledger = env.ledger().sequence();

        Self::persist_alert(&env, config_id, &config);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("update")),
            (config_id, config.owner.clone(), active),
        );
        Ok(())
    }

    /// Update the webhook hash for an existing alert.
    ///
    /// Takes effect immediately and discards any rotation staged by
    /// [`AlertRegistry::propose_webhook`], so a later
    /// [`AlertRegistry::confirm_webhook`] returns
    /// [`ContractError::NoPendingWebhook`] instead of reverting this update.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn update_webhook(
        env: Env,
        caller: Address,
        config_id: u64,
        webhook_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        config.webhook_hash = webhook_hash;
        // A direct update supersedes any in-flight rotation; otherwise a later
        // confirm_webhook would promote the stale staged hash over this one.
        config.pending_webhook_hash = None;
        config.updated_at = env.ledger().timestamp();
        config.updated_ledger = env.ledger().sequence();

        Self::persist_alert(&env, config_id, &config);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("webhook")),
            (config_id, caller),
        );
        Ok(())
    }

    /// Stage a replacement webhook hash without taking it live.
    ///
    /// The alert keeps delivering to its current `webhook_hash` until
    /// [`AlertRegistry::confirm_webhook`] promotes the staged value, so a mistyped or
    /// unreachable endpoint can never silently displace a working one. Calling
    /// this again before confirming overwrites the staged value.
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
    /// Emits `(Symbol("alert"), Symbol("wh_prop"))` with data `(id: u64, caller: Address)`.
    pub fn propose_webhook(
        env: Env,
        caller: Address,
        config_id: u64,
        webhook_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        Self::assert_owner(&config, &caller)?;

        // The live hash is deliberately left untouched until confirmation.
        config.pending_webhook_hash = Some(webhook_hash);

        Self::persist_alert(&env, config_id, &config);

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
        config.updated_ledger = env.ledger().sequence();

        Self::persist_alert(&env, config_id, &config);

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
        config.updated_ledger = env.ledger().sequence();

        Self::persist_alert(&env, config_id, &config);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("wh_cancel")),
            (config_id, caller),
        );
        Ok(())
    }

    /// Extend the TTL of an alert and its indexes without modifying any data.
    ///
    /// Unlike [`Self::bump_alert`], this is owner-authenticated and leaves
    /// `updated_at` and `updated_ledger` alone, so renewing storage never looks like an edit to
    /// downstream consumers polling `get_alerts_modified_since` or `get_alerts_modified_since_ledger`.
    /// Unlike [`AlertRegistry::bump_alert`], this is owner-authenticated and leaves
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

        Self::touch_alert(&env, config_id, &config, DEFAULT_TTL);

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
    /// Returns [`ContractError::LabelTooLong`] if `label` exceeds 128 bytes.
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
        config.updated_ledger = env.ledger().sequence();

        Self::persist_alert(&env, config_id, &config);

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
    /// If the alert's record no longer exists but its ID is still in the
    /// caller's owner index (the record expired instead of being removed),
    /// the dangling index entry is cleaned up instead, releasing the quota
    /// slot it was holding.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must also be
    /// the original owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` is neither an
    /// existing alert nor a dangling entry in the caller's owner index.
    /// Returns [`ContractError::Unauthorized`] if the caller is not authorized for this operation.
    pub fn remove_alert(env: Env, caller: Address, config_id: u64) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let Some(config) = env
            .storage()
            .persistent()
            .get::<DataKey, AlertConfig>(&DataKey::Alert(config_id))
        else {
            if Self::owner_index(&env, &caller).contains(config_id) {
                Self::drop_dangling_ids(&env, &caller, &vec![&env, config_id]);
                env.events().publish(
                    (symbol_short!("alert"), symbol_short!("remove")),
                    (config_id, caller),
                );
                return Ok(());
            }
            return Err(ContractError::AlertNotFound);
        };

        Self::assert_owner(&config, &caller)?;
        Self::remove_alert_record(&env, &config, config_id, &caller);
        Ok(())
    }

    /// Drop IDs from `owner`'s index whose alert record no longer exists
    /// (it expired instead of being removed), decrementing the owner's live
    /// counter so the quota slots they held are released.
    ///
    /// Callable by anyone and requires no auth: it only removes entries that
    /// point at nothing. [`AlertRegistry::register_alert`] also runs it
    /// automatically when an owner is at the per-owner limit.
    ///
    /// # Returns
    /// The number of dangling IDs removed.
    ///
    /// # Errors
    /// Returns [`ContractError::Paused`] while the contract is paused.
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("pruned"))` with data
    /// `(owner: Address, count: u32)` when at least one ID was removed.
    pub fn prune_expired_alerts(env: Env, owner: Address) -> Result<u32, ContractError> {
        Self::assert_not_paused(&env)?;
        Ok(Self::prune_owner_index(&env, &owner))
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
    /// Unlike [`AlertRegistry::remove_alert_by_admin`], the alert config and its
    /// indexes are left intact — only the `active` flag is cleared — so
    /// history is preserved for e.g. spam/abuse moderation.
    ///
    /// The alert is also **suspended**: its owner cannot reactivate it (with
    /// [`AlertRegistry::update_alert`]) until an admin calls
    /// [`AlertRegistry::unlock_alert_by_admin`].
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
        Self::assert_not_paused(&env)?;

        let mut config: AlertConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)?;

        config.active = false;
        config.updated_at = env.ledger().timestamp();
        config.updated_ledger = env.ledger().sequence();

        env.storage()
            .persistent()
            .set(&DataKey::AdminSuspended(config_id), &true);
        Self::persist_alert(&env, config_id, &config);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("admin_off")),
            (config_id, admin),
        );
        Ok(())
    }

    /// Lift an admin suspension so the owner can reactivate the alert
    /// (admin only). The alert itself stays inactive until the owner
    /// reactivates it. Unlocking an alert that is not suspended does nothing.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `admin`.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::NotInitialized`] if the contract has not been initialized.
    /// Returns [`ContractError::Unauthorized`] if the caller is not the admin.
    /// Returns [`ContractError::Paused`] while the contract is paused.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("admin_on"))` with data `(id: u64, admin: Address)`
    /// when a suspension was lifted.
    pub fn unlock_alert_by_admin(
        env: Env,
        admin: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;
        Self::assert_not_paused(&env)?;
        Self::load_alert(&env, config_id)?;

        let key = DataKey::AdminSuspended(config_id);
        if env.storage().persistent().has(&key) {
            env.storage().persistent().remove(&key);
            env.events().publish(
                (symbol_short!("alert"), symbol_short!("admin_on")),
                (config_id, admin),
            );
        }
        Ok(())
    }

    /// Whether an admin has suspended the alert (see
    /// [`AlertRegistry::deactivate_alert_by_admin`]).
    #[must_use]
    pub fn is_alert_suspended(env: Env, config_id: u64) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::AdminSuspended(config_id))
    }

    /// Propose transferring an alert to `new_owner`.
    ///
    /// Nothing changes until `new_owner` calls
    /// [`AlertRegistry::accept_alert_transfer`], so nobody can be made the owner
    /// of alerts they did not agree to take (which would otherwise fill their
    /// per-owner quota and pollute their alert list). Proposing again replaces
    /// any earlier pending transfer. The proposal expires after
    /// [`ALERT_TRANSFER_EXPIRY_LEDGERS`] ledgers.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// current owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the current owner.
    /// Returns [`ContractError::InvalidTransferRecipient`] if `new_owner` already owns the alert.
    /// Returns [`ContractError::Paused`] while the contract is paused.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("xfer_prop"))` with data
    /// `(id: u64, owner: Address, new_owner: Address, expires_at_ledger: u32)`.
    pub fn propose_alert_transfer(
        env: Env,
        caller: Address,
        config_id: u64,
        new_owner: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let config = Self::load_alert(&env, config_id)?;
        Self::assert_owner(&config, &caller)?;
        if new_owner == config.owner {
            return Err(ContractError::InvalidTransferRecipient);
        }

        let expires_at_ledger = env
            .ledger()
            .sequence()
            .saturating_add(ALERT_TRANSFER_EXPIRY_LEDGERS);
        let key = DataKey::PendingTransfer(config_id);
        env.storage().persistent().set(
            &key,
            &PendingAlertTransfer {
                new_owner: new_owner.clone(),
                expires_at_ledger,
            },
        );
        env.storage().persistent().extend_ttl(
            &key,
            ALERT_TRANSFER_EXPIRY_LEDGERS,
            ALERT_TRANSFER_EXPIRY_LEDGERS,
        );

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("xfer_prop")),
            (config_id, caller, new_owner, expires_at_ledger),
        );
        Ok(())
    }

    /// Accept a pending transfer, becoming the alert's owner.
    ///
    /// Updates the [`AlertConfig::owner`] field and migrates the alert ID from
    /// the old owner's [`DataKey::OwnerIndex`] to the new owner's.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `new_owner`, who must be
    /// the recipient named in the pending transfer.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::NoPendingTransfer`] if no transfer is pending.
    /// Returns [`ContractError::Unauthorized`] if `new_owner` is not the named recipient.
    /// Returns [`ContractError::TransferExpired`] if the transfer is past its expiry ledger.
    /// Returns [`ContractError::OwnerAlertLimitExceeded`] if `new_owner` is already at the
    /// per-owner alert limit.
    /// Returns [`ContractError::Paused`] while the contract is paused.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("transfer"))` with data
    /// `(id: u64, old_owner: Address, new_owner: Address)`.
    pub fn accept_alert_transfer(
        env: Env,
        new_owner: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        new_owner.require_auth();
        Self::assert_not_paused(&env)?;

        let mut config = Self::load_alert(&env, config_id)?;
        let pending = Self::pending_transfer(&env, config_id)?;
        if pending.new_owner != new_owner {
            return Err(ContractError::Unauthorized);
        }
        if env.ledger().sequence() > pending.expires_at_ledger {
            return Err(ContractError::TransferExpired);
        }
        // Receiving an alert counts against the recipient's quota exactly like
        // registering one; otherwise colluding accounts could pile any number
        // of alerts onto one owner (#200).
        Self::assert_per_owner_limit(&env, &new_owner)?;

        let old_owner = config.owner.clone();
        config.owner = new_owner.clone();
        config.updated_at = env.ledger().timestamp();
        config.updated_ledger = env.ledger().sequence();

        Self::clear_pending_transfer(&env, config_id);
        Self::remove_from_owner_index(&env, &old_owner, config_id);
        Self::push_owner_index(&env, &new_owner, config_id)?;
        Self::persist_alert(&env, config_id, &config);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("transfer")),
            (config_id, old_owner, new_owner),
        );
        Ok(())
    }

    /// Decline a pending transfer as its recipient. The alert stays with its
    /// current owner.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `new_owner`, who must be
    /// the recipient named in the pending transfer.
    /// # Errors
    /// Returns [`ContractError::NoPendingTransfer`] if no transfer is pending.
    /// Returns [`ContractError::Unauthorized`] if `new_owner` is not the named recipient.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("xfer_rej"))` with data `(id: u64, new_owner: Address)`.
    pub fn reject_alert_transfer(
        env: Env,
        new_owner: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        new_owner.require_auth();
        Self::assert_not_paused(&env)?;

        let pending = Self::pending_transfer(&env, config_id)?;
        if pending.new_owner != new_owner {
            return Err(ContractError::Unauthorized);
        }
        Self::clear_pending_transfer(&env, config_id);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("xfer_rej")),
            (config_id, new_owner),
        );
        Ok(())
    }

    /// Withdraw a pending transfer as the alert's owner.
    ///
    /// # Auth
    /// Requires a valid Stellar auth signature from `caller`, who must be the
    /// current owner of the alert.
    /// # Errors
    /// Returns [`ContractError::AlertNotFound`] if `config_id` does not identify an existing alert.
    /// Returns [`ContractError::Unauthorized`] if `caller` is not the current owner.
    /// Returns [`ContractError::NoPendingTransfer`] if no transfer is pending.
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("xfer_can"))` with data `(id: u64, owner: Address)`.
    pub fn cancel_alert_transfer(
        env: Env,
        caller: Address,
        config_id: u64,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;

        let config = Self::load_alert(&env, config_id)?;
        Self::assert_owner(&config, &caller)?;
        Self::pending_transfer(&env, config_id)?;
        Self::clear_pending_transfer(&env, config_id);

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("xfer_can")),
            (config_id, caller),
        );
        Ok(())
    }

    /// The transfer pending for an alert, if any. An expired transfer is still
    /// returned (check `expires_at_ledger`); it can no longer be accepted.
    #[must_use]
    pub fn get_pending_alert_transfer(env: Env, config_id: u64) -> Option<PendingAlertTransfer> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingTransfer(config_id))
    }

    /// Register multiple alert configs in a single call.
    ///
    /// Each input is validated and authorized exactly as
    /// [`AlertRegistry::register_alert`] would, and each successful registration emits
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
    /// Returns the same errors as [`AlertRegistry::register_alert`] for the failing item.
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
    /// Each ID is validated and authorized exactly as [`AlertRegistry::remove_alert`]
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
        Self::assert_not_paused(&env)?;

        for i in 0..config_ids.len() {
            let config_id = config_ids.get(i).unwrap();
            if config_ids.iter().take(i).any(|id| id == config_id) {
                continue;
            }
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
    /// Callers should request at most [`MAX_TTL`] ledgers. Larger values are
    /// currently capped at `MAX_TTL`, and the TTL actually applied is reported
    /// in the `alert.bump` event; see [`MAX_TTL`] for why callers should not
    /// rely on the capping.
    ///
    /// This is the primary mechanism for keeping long-lived alerts alive
    /// without modifying their content.  Unlike `update_alert`, this function
    /// does **not** require the caller to be the alert owner — any address may
    /// bump an alert's TTL (e.g. an off-chain keeper service).
    ///
    /// # Arguments
    /// * `config_id` - ID of the alert to extend.
    /// * `ttl`       - Desired TTL in ledgers, at most [`MAX_TTL`].
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

        Self::touch_alert(&env, config_id, &config, effective_ttl);

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
    /// Equivalent to [`AlertRegistry::get_alerts_for_contract`] but filters out any entries
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
    /// of [`AlertRegistry::get_alerts_by_owner`] when only the IDs are needed (e.g. an
    /// existence check or a count) so callers don't pay the cost of
    /// deserializing every full [`AlertConfig`].
    ///
    /// Unlike [`AlertRegistry::get_alerts_by_owner`], this is not subject to
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
    /// independently of the record (and `accept_alert_transfer` rewrites
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
    /// The number of alerts that were deactivated (`0` if the owner had no
    /// active alerts).
    ///
    /// # Errors
    /// Returns [`ContractError::Paused`] while the contract is paused, like
    /// every other mutator, so a paused call is never mistaken for an owner
    /// with nothing to deactivate.
    ///
    /// # Events
    /// Emits `(Symbol("alert"), Symbol("bulk_off"))` with data
    /// `(caller: Address, count: u32)` when at least one alert was deactivated.
    /// No event is emitted if `count` is `0`.
    pub fn deactivate_all_alerts(env: Env, caller: Address) -> Result<u32, ContractError> {
        caller.require_auth();
        Self::assert_not_paused(&env)?;
        let ids = Self::owner_index(&env, &caller);
        let mut count: u32 = 0;
        for id in ids.iter() {
            if let Some(mut cfg) = env
                .storage()
                .persistent()
                .get::<DataKey, AlertConfig>(&DataKey::Alert(id))
            {
                if cfg.active {
                    cfg.active = false;
                    cfg.updated_at = env.ledger().timestamp();
                    Self::persist_alert(&env, id, &cfg);
                    cfg.updated_ledger = env.ledger().sequence();
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
            env.events().publish(
                (symbol_short!("alert"), symbol_short!("bulk_off")),
                (caller, count),
            );
        }
        Ok(count)
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
    /// Returns [`ContractError::ContractAlertLimitExceeded`] if `new_target` is
    /// already at the per-contract alert limit.
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

        // Moving into a target counts against its limit exactly like
        // registering there would; otherwise alerts registered against
        // throwaway targets could all be retargeted at one contract (#199).
        // Retargeting to the current target takes no extra slot.
        if new_target != config.target_contract {
            Self::assert_per_contract_limit(&env, &new_target)?;
        }

        let old_target = config.target_contract.clone();
        config.target_contract = new_target.clone();
        config.updated_at = env.ledger().timestamp();
        config.updated_ledger = env.ledger().sequence();

        // Migrate the contract index before persisting, so the refresh in
        // persist_alert extends the new target's index.
        Self::remove_from_contract_index(&env, &old_target, config_id);
        Self::push_contract_index(&env, &new_target, config_id)?;
        Self::persist_alert(&env, config_id, &config);
        // A recipient agreed to take the alert as it was; a different target
        // is a different alert, so any pending transfer must be re-proposed.
        Self::clear_pending_transfer(&env, config_id);

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
    /// Because multiple ledgers can share the same close-time second, timestamp-based
    /// synchronization may produce duplicates or miss changes across ledgers closed in
    /// the same second. For unambiguous monotonic synchronization, prefer
    /// [`Self::get_alerts_modified_since_ledger`].
    ///
    /// The scan cost of a single call is bounded by `limit`, not by the total
    /// size of the registry, so callers should page through with a bounded
    /// `limit` (see [`AlertRegistry::get_global_alert_limit`] for an admin-settable ceiling
    /// on total registry size) rather than requesting the whole ID space in
    /// one call. Callers that need every alert should page repeatedly,
    /// advancing `offset` by `limit` each call until fewer than `limit`
    /// results are returned.
    #[must_use]
    pub fn get_alerts_modified_since(env: Env, since: u64, offset: u32, limit: u32) -> Vec<AlertConfig> {
        let total: u64 = env
            .storage()
            .instance()
            .get(&instance_key::NEXT_ID)
            .unwrap_or(0u64);

        let range_start = u64::from(offset).min(total);
        let range_end = u64::from(offset)
            .saturating_add(u64::from(limit.min(MAX_PAGE_SIZE)))
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

    /// Return all alert configs whose `updated_ledger` sequence number is greater
    /// than or equal to `since_ledger`.
    ///
    /// This provides unambiguous **incremental sync** for watcher nodes keyed on
    /// the monotonic ledger sequence number rather than timestamps. Because several
    /// ledgers can share the same close-time second, timestamp-based polling with
    /// `since = T` gets duplicates while `since = T + 1` can miss changes occurring in
    /// later ledgers closed within the same second. Monotonic ledger sequences eliminate
    /// this ambiguity.
    ///
    /// # Recommended Sync Loop
    /// 1. Initialize `cursor_ledger = 0` (or the last-synced ledger sequence).
    /// 2. For each polling cycle:
    ///    a. Call `get_alerts_modified_since_ledger(env, cursor_ledger, offset, limit)`
    ///       paginating by advancing `offset += limit` until an empty page or fewer than
    ///       `limit` items are returned.
    ///    b. For each returned alert, update local state and track the highest ledger
    ///       seen: `max_ledger = max(max_ledger, alert.updated_ledger)`.
    ///    c. After finishing the registry scan, advance the cursor:
    ///       `cursor_ledger = max(cursor_ledger, max_ledger.saturating_add(1))`.
    ///
    /// # Arguments
    /// * `since_ledger` - Monotonic ledger sequence number (inclusive lower bound). Pass `0`
    ///   to retrieve every alert that is currently stored.
    /// * `offset` - Number of IDs to skip from the start of the ID space.
    /// * `limit` - Maximum number of IDs to scan starting at `offset`.
    ///
    /// # Returns
    /// A `Vec<AlertConfig>` containing every live alert in the ID range
    /// `[offset, offset + limit)` (clamped to the current alert count) with
    /// `updated_ledger >= since_ledger`. Alerts that have been removed are silently omitted.
    ///
    /// # Note
    /// The scan cost of a single call is bounded by `limit`, not by the total
    /// size of the registry, so callers should page through with a bounded
    /// `limit` (see [`get_global_alert_limit`] for an admin-settable ceiling
    /// on total registry size) rather than requesting the whole ID space in
    /// one call.
    #[must_use]
    pub fn get_alerts_modified_since_ledger(
        env: Env,
        since_ledger: u32,
        offset: u32,
        limit: u32,
    ) -> Vec<AlertConfig> {
        let total: u64 = env
            .storage()
            .instance()
            .get(&instance_key::NEXT_ID)
            .unwrap_or(0u64);

        let range_start = u64::from(offset).min(total);
        let range_end = u64::from(offset)
            .saturating_add(u64::from(limit.min(MAX_PAGE_SIZE)))
            .min(total);

        let mut out: Vec<AlertConfig> = vec![&env];
        for id in range_start..range_end {
            if let Some(cfg) = env
                .storage()
                .persistent()
                .get::<DataKey, AlertConfig>(&DataKey::Alert(id))
            {
                if cfg.updated_ledger >= since_ledger {
                    out.push_back(cfg);
                }
            }
        }
        out
    }

    /// Get the total number of alerts ever registered.
    ///
    /// This is a **monotonic counter** — it only increases and is never
    /// decremented when alerts are removed. Use [`AlertRegistry::get_non_removed_alert_count`]
    /// if you need the number of currently live (non-removed) alerts for a
    /// given owner, or [`AlertRegistry::get_active_alert_count`] for the number that are
    /// still active.
    #[must_use]
    pub fn get_alert_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&instance_key::NEXT_ID)
            .unwrap_or(0u64)
    }

    /// Get the number of currently active alerts owned by `owner`.
    ///
    /// Unlike [`AlertRegistry::get_alert_count`], this reflects removals and only counts
    /// alerts with `active == true` — deactivated-but-not-removed alerts are
    /// excluded, so the result matches the `active` flag of the alerts
    /// returned by [`AlertRegistry::get_alerts_by_owner`].
    ///
    /// Scans the owner's [`DataKey::OwnerIndex`] and reads the cheap
    /// [`DataKey::AlertActive`] flag for each entry, so it reflects both
    /// removals and deactivations. If you only need the number of live
    /// (non-removed) alerts regardless of the `active` flag, use
    /// [`AlertRegistry::get_non_removed_alert_count`], which is an O(1) lookup.
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
    /// Unlike [`AlertRegistry::get_active_alert_count`], this does **not** filter by
    /// the `active` flag: deactivated-but-not-removed alerts still count.
    ///
    /// Backed by a running counter maintained incrementally by
    /// [`AlertRegistry::push_owner_index`]/[`AlertRegistry::remove_from_owner_index`], so this
    /// is an O(1) lookup regardless of how many alerts `owner` has ever
    /// registered — it never rescans the owner's index.
    #[must_use]
    pub fn get_non_removed_alert_count(env: Env, owner: Address) -> u32 {
        Self::owner_live_count(&env, &owner)
    }

    /// Get the number of live (non-removed, unexpired) alerts targeting
    /// `target_contract`, aggregated across every contributing owner.
    ///
    /// Keyed by target contract rather than owner. Unlike
    /// [`AlertRegistry::get_active_alert_count`], this does **not** filter by the
    /// `active` flag: deactivated-but-not-removed alerts still count.
    pub fn get_active_contract_alert_count(env: Env, target_contract: Address) -> u32 {
        let ids = Self::contract_index(&env, &target_contract);
        let mut count: u32 = 0;
        for id in ids.iter() {
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
        let maybe_registry: Option<Address> = env.storage().instance().get(&instance_key::WATCHREG);

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
            .get(&instance_key::PAUSED)
            .unwrap_or(false);
        if paused {
            return Err(ContractError::Paused);
        }
        Ok(())
    }

    fn assert_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if !env.storage().instance().has(&instance_key::ADMIN) {
            return Err(ContractError::NotInitialized);
        }
        let admin: Address = env.storage().instance().get(&instance_key::ADMIN).unwrap();
        if admin == *caller {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn assert_per_owner_limit(env: &Env, owner: &Address) -> Result<(), ContractError> {
        let limit = Self::get_per_owner_alert_limit(env.clone());
        if limit == 0 || Self::owner_live_count(env, owner) < limit {
            return Ok(());
        }
        // Only pay for a scan of the owner's index when they are at the limit:
        // expired alerts may be holding slots that should be released.
        Self::prune_owner_index(env, owner);
        if Self::owner_live_count(env, owner) >= limit {
            return Err(ContractError::OwnerAlertLimitExceeded);
        }
        Ok(())
    }

    /// Find IDs in `owner`'s index whose alert record no longer exists and
    /// drop them. Returns how many were dropped.
    fn prune_owner_index(env: &Env, owner: &Address) -> u32 {
        let mut dangling: Vec<u64> = vec![env];
        for id in Self::owner_index(env, owner).iter() {
            if !env.storage().persistent().has(&DataKey::Alert(id)) {
                dangling.push_back(id);
            }
        }
        if dangling.is_empty() {
            return 0;
        }
        Self::drop_dangling_ids(env, owner, &dangling);
        env.events().publish(
            (symbol_short!("alert"), symbol_short!("pruned")),
            (owner.clone(), dangling.len()),
        );
        dangling.len()
    }

    /// Remove `ids` (whose records are gone) from `owner`'s index, decrement
    /// the live counter accordingly, and clear any leftover per-alert entries.
    ///
    /// The contract index cannot be cleaned here: the target contract was only
    /// recorded in the expired record. Its dangling IDs are harmless, since
    /// contract-level counts check that each record exists.
    fn drop_dangling_ids(env: &Env, owner: &Address, ids: &Vec<u64>) {
        let storage = env.storage().persistent();
        let mut kept: Vec<u64> = vec![env];
        let mut dropped: u32 = 0;
        for id in Self::owner_index(env, owner).iter() {
            if ids.contains(id) {
                dropped += 1;
            } else {
                kept.push_back(id);
            }
        }
        for id in ids.iter() {
            storage.remove(&DataKey::AlertActive(id));
            storage.remove(&DataKey::PendingTransfer(id));
            storage.remove(&DataKey::AdminSuspended(id));
        }
        storage.set(&DataKey::OwnerIndex(owner.clone()), &kept);
        storage.extend_ttl(&DataKey::OwnerIndex(owner.clone()), DEFAULT_TTL, DEFAULT_TTL);
        let count = Self::owner_live_count(env, owner);
        Self::set_owner_live_count(env, owner, count.saturating_sub(dropped));
        Self::set_live_alert_count(
            env,
            Self::get_live_alert_count(env).saturating_sub(u64::from(dropped)),
        );
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

    /// Reject registration once the number of currently live alerts reaches
    /// the configured global ceiling. A limit of `0` means no ceiling.
    fn assert_global_alert_limit(env: &Env) -> Result<(), ContractError> {
        let limit = Self::get_global_alert_limit(env.clone());
        if limit > 0 && Self::get_live_alert_count(env) >= u64::from(limit) {
            return Err(ContractError::GlobalAlertLimitExceeded);
        }
        Ok(())
    }

    fn load_alert(env: &Env, config_id: u64) -> Result<AlertConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Alert(config_id))
            .ok_or(ContractError::AlertNotFound)
    }

    fn pending_transfer(env: &Env, config_id: u64) -> Result<PendingAlertTransfer, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingTransfer(config_id))
            .ok_or(ContractError::NoPendingTransfer)
    }

    fn clear_pending_transfer(env: &Env, config_id: u64) {
        env.storage()
            .persistent()
            .remove(&DataKey::PendingTransfer(config_id));
    }

    fn remove_alert_record(env: &Env, config: &AlertConfig, config_id: u64, caller: &Address) {
        Self::clear_pending_transfer(env, config_id);
        env.storage()
            .persistent()
            .remove(&DataKey::AdminSuspended(config_id));
        env.storage()
            .persistent()
            .remove(&DataKey::Alert(config_id));
        env.storage()
            .persistent()
            .remove(&DataKey::AlertActive(config_id));

        Self::remove_from_owner_index(env, &config.owner, config_id);
        Self::remove_from_contract_index(env, &config.target_contract, config_id);
        Self::set_live_alert_count(
            env,
            Self::get_live_alert_count(env).saturating_sub(1),
        );

        env.events().publish(
            (symbol_short!("alert"), symbol_short!("remove")),
            (config_id, caller.clone()),
        );
    }

    /// Write `config` (and its cheap [`DataKey::AlertActive`] mirror) under
    /// `config_id`, then refresh every entry the alert depends on via
    /// [`AlertRegistry::touch_alert`].
    ///
    /// Every mutator that rewrites an alert goes through here, so the
    /// config/`AlertActive` pair can never drift apart and no mutator can
    /// forget one of the TTL extensions.
    ///
    /// The owner and contract indexes must already contain `config_id`, so
    /// callers that move an alert between indexes update them first.
    fn persist_alert(env: &Env, config_id: u64, config: &AlertConfig) {
        let storage = env.storage().persistent();
        storage.set(&DataKey::Alert(config_id), config);
        storage.set(&DataKey::AlertActive(config_id), &config.active);
        Self::touch_alert(env, config_id, config, DEFAULT_TTL);
    }

    /// Extend the TTL of an alert and of every entry it depends on to `ttl`
    /// ledgers: [`DataKey::Alert`], [`DataKey::AlertActive`], the owner's
    /// [`DataKey::OwnerIndex`] and [`DataKey::OwnerLiveCount`], and the
    /// target's [`DataKey::ContractIndex`].
    ///
    /// The owner counter is only extended when present: it may still sit
    /// under its pre-rename key (migrated lazily by `owner_live_count`) or
    /// have expired, and extending a missing entry would abort the call.
    fn touch_alert(env: &Env, config_id: u64, config: &AlertConfig, ttl: u32) {
        let storage = env.storage().persistent();
        storage.extend_ttl(&DataKey::Alert(config_id), ttl, ttl);
        storage.extend_ttl(&DataKey::AlertActive(config_id), ttl, ttl);
        storage.extend_ttl(&DataKey::OwnerIndex(config.owner.clone()), ttl, ttl);
        storage.extend_ttl(
            &DataKey::ContractIndex(config.target_contract.clone()),
            ttl,
            ttl,
        );
        let live_count_key = DataKey::OwnerLiveCount(config.owner.clone());
        if storage.has(&live_count_key) {
            storage.extend_ttl(&live_count_key, ttl, ttl);
        }
        let suspended_key = DataKey::AdminSuspended(config_id);
        if storage.has(&suspended_key) {
            storage.extend_ttl(&suspended_key, ttl, ttl);
        }
    }

    /// Atomically read and increment the global alert ID counter.
    ///
    /// Returns the current value before incrementing, so the first ID is `0`.
    fn next_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&instance_key::NEXT_ID)
            .unwrap_or(0u64);
        env.storage()
            .instance()
            .set(&instance_key::NEXT_ID, &(id + 1));
        Self::extend_instance_ttl(env);
        id
    }

    fn get_live_alert_count(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&instance_key::LIVE)
            .unwrap_or(0u64)
    }

    fn set_live_alert_count(env: &Env, count: u64) {
        env.storage().instance().set(&instance_key::LIVE, &count);
        Self::extend_instance_ttl(env);
    }

    /// Keep the instance entry (admin, counter, limits, pause flag, watcher
    /// registry) alive. Called on every write to instance storage; see
    /// [`AlertRegistry::bump_instance_ttl`] for deployments that go quiet.
    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
    ///
    /// Counters written before the `OwnerActiveCount` → `OwnerLiveCount`
    /// rename live under the legacy key. On a miss the legacy entry is moved
    /// to the new key (and deleted), so each owner is migrated exactly once
    /// and no counter is lost across the upgrade.
    fn owner_live_count(env: &Env, owner: &Address) -> u32 {
        let storage = env.storage().persistent();
        if let Some(count) = storage.get::<DataKey, u32>(&DataKey::OwnerLiveCount(owner.clone())) {
            return count;
        }

        let legacy_key = Self::legacy_owner_active_count_key(env, owner);
        match storage.get::<_, u32>(&legacy_key) {
            Some(count) => {
                storage.remove(&legacy_key);
                Self::set_owner_live_count(env, owner, count);
                count
            }
            None => 0,
        }
    }

    /// Storage key the counter used before the rename: the encoding of the
    /// former `DataKey::OwnerActiveCount(owner)` variant, i.e. the vector
    /// `[Symbol("OwnerActiveCount"), owner]`.
    fn legacy_owner_active_count_key(env: &Env, owner: &Address) -> (soroban_sdk::Symbol, Address) {
        (
            soroban_sdk::Symbol::new(env, "OwnerActiveCount"),
            owner.clone(),
        )
    }

    /// Persist the running per-owner live-alert counter with a refreshed TTL.
    fn set_owner_live_count(env: &Env, owner: &Address, count: u32) {
        env.storage()
            .persistent()
            .set(&DataKey::OwnerLiveCount(owner.clone()), &count);
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerLiveCount(owner.clone()),
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
        let count = Self::owner_live_count(env, owner);
        Self::set_owner_live_count(env, owner, count + 1);
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
            let count = Self::owner_live_count(env, owner);
            Self::set_owner_live_count(env, owner, count.saturating_sub(1));
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

    /// Like [`AlertRegistry::configs_for_ids`] but only includes entries where `active == true`.
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
        let last = offset
            .saturating_add(limit.min(MAX_PAGE_SIZE))
            .min(count);
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _, Ledger as _},
        vec, Env, FromVal, String, Symbol,
    };

    /// A webhook hash (32-byte SHA-256 digest) with every byte set to `c`;
    /// vary `c` when a test needs two hashes that must differ.
    fn hash64c(env: &Env, c: char) -> BytesN<32> {
        BytesN::from_array(env, &[c as u8; 32])
    }

    /// The default valid 64-character webhook hash.
    fn hash64(env: &Env) -> BytesN<32> {
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

        let cfg = client.get_alert(&owner, &id).unwrap();
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

        let cfg = client.get_alert(&owner, &id).unwrap();
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
        assert!(client.get_alert(&owner, &id).is_none());
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
        assert!(client.get_alert(&owner, &id).is_none());
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

    // ── Feature: global alert-count ceiling (#38) ─────────────────────────

    #[test]
    fn test_global_alert_limit_defaults_to_zero_unlimited() {
        let (_env, client) = setup();
        assert_eq!(client.get_global_alert_limit(), 0u32);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #13)")]
    fn test_global_alert_limit_enforced_across_owners() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_global_alert_limit(&admin, &2u32);

        let target = Address::generate(&env);
        // Two different owners share the same global ceiling.
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        // Third registration, from yet another owner, exceeds the ceiling.
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert3"),
            &hash64c(&env, '3'),
            &vec![&env, str(&env, "rule:mint")],
        );
    }

    #[test]
    fn test_global_alert_limit_is_released_by_removal() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_global_alert_limit(&admin, &1u32);

        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert1"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );
        client.remove_alert(&owner, &id);

        let replacement = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );
        assert_eq!(replacement, 1);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_set_global_alert_limit_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.set_global_alert_limit(&admin, &5u32);
    }

    #[test]
    fn test_set_global_alert_limit_non_admin_rejected() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        client.initialize(&admin);

        assert_eq!(
            client
                .try_set_global_alert_limit(&attacker, &5u32)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // ── Feature: per-contract alert-count ceiling (#40) ────────────────────

    #[test]
    fn test_per_contract_alert_limit_defaults_to_zero_unlimited() {
        let (_env, client) = setup();
        assert_eq!(client.get_per_contract_alert_limit(), 0u32);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #14)")]
    fn test_per_contract_alert_limit_enforced_across_owners() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_per_contract_alert_limit(&admin, &2u32);

        let target = Address::generate(&env);
        // Two different owners contribute to the same target contract.
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        // Third registration against the same target, from yet another
        // owner, exceeds the per-contract ceiling.
        client.register_alert(
            &Address::generate(&env),
            &target,
            &str(&env, "Alert3"),
            &hash64c(&env, '3'),
            &vec![&env, str(&env, "rule:mint")],
        );
    }

    #[test]
    fn test_per_contract_alert_limit_independent_per_contract() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_per_contract_alert_limit(&admin, &1u32);

        let target_a = Address::generate(&env);
        let target_b = Address::generate(&env);

        // One alert against target_a fills its ceiling...
        client.register_alert(
            &Address::generate(&env),
            &target_a,
            &str(&env, "Alert1"),
            &hash64c(&env, '1'),
            &vec![&env, str(&env, "rule:transfer")],
        );

        // ...but target_b's own ceiling is untouched.
        client.register_alert(
            &Address::generate(&env),
            &target_b,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );

        assert_eq!(client.get_active_contract_alert_count(&target_a), 1u32);
        assert_eq!(client.get_active_contract_alert_count(&target_b), 1u32);
    }

    #[test]
    fn test_per_contract_alert_limit_freed_by_removal() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.set_per_contract_alert_limit(&admin, &1u32);

        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert1"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );

        // Unlike the global ceiling, the per-contract limit tracks currently
        // active alerts, so removing one reopens room for the target.
        client.remove_alert(&owner, &id);
        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert2"),
            &hash64c(&env, '2'),
            &vec![&env, str(&env, "rule:mint")],
        );
        assert_eq!(client.get_active_contract_alert_count(&target), 1u32);
    }

    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_set_per_contract_alert_limit_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.set_per_contract_alert_limit(&admin, &5u32);
    }

    #[test]
    fn test_set_per_contract_alert_limit_non_admin_rejected() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        client.initialize(&admin);

        assert_eq!(
            client
                .try_set_per_contract_alert_limit(&attacker, &5u32)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    #[test]
    fn test_set_per_contract_alert_limit_emits_admin_limit_event() {
        use soroban_sdk::{symbol_short, testutils::Events as _};

        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        client.set_per_contract_alert_limit(&admin, &7u32);

        let events = env.events().all();
        let limit_event = events
            .iter()
            .find(|(_, topics, _)| {
                topics.len() == 2
                    && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("admin")
                    && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("limit")
            })
            .expect("admin.limit event must be emitted");

        let (_, _, data) = limit_event;
        let (kind, emitted_limit): (Symbol, u32) = soroban_sdk::FromVal::from_val(&env, &data);
        assert_eq!(kind, symbol_short!("contract"));
        assert_eq!(emitted_limit, 7u32);
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
        let (env, client) = setup();
        assert!(client.get_alert(&Address::generate(&env), &999u64).is_none());
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

    // 8b. get_alert_ids_by_owner — thin ID-only wrapper over the owner index (#35)
    #[test]
    fn test_get_alert_ids_by_owner() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let other = Address::generate(&env);
        let target = Address::generate(&env);

        assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 0);

        let id1 = client.register_alert(
            &owner,
            &target,
            &str(&env, "A1"),
            &hash64(&env),
            &vec![&env],
        );
        let id2 = client.register_alert(
            &owner,
            &target,
            &str(&env, "A2"),
            &hash64(&env),
            &vec![&env],
        );

        let ids = client.get_alert_ids_by_owner(&owner);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.get(0).unwrap(), id1);
        assert_eq!(ids.get(1).unwrap(), id2);

        // Unrelated owner still sees an empty list.
        assert_eq!(client.get_alert_ids_by_owner(&other).len(), 0);
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

    // 14b. Watcher registry configured — get_alert, get_alert_active, and
    // get_active_alerts_for_contract reject a non-watcher the same way the
    // other gated query functions do (#42).
    #[test]
    #[cfg(feature = "testutils")]
    fn test_watcher_registry_get_alert_family_rejects_non_watcher() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        let stranger = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        watcher_client.initialize(&admin);
        watcher_client.register_watcher(&admin, &watcher);

        alert_client.initialize(&admin);
        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);

        let id = alert_client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Registered watcher can use all three.
        assert!(alert_client.get_alert(&watcher, &id).is_some());
        assert_eq!(alert_client.get_alert_active(&watcher, &id), Some(true));
        assert_eq!(
            alert_client
                .get_active_alerts_for_contract(&watcher, &target)
                .len(),
            1
        );

        // A stranger is rejected on all three.
        assert_eq!(
            alert_client
                .try_get_alert(&stranger, &id)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );
        assert_eq!(
            alert_client
                .try_get_alert_active(&stranger, &id)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );
        assert_eq!(
            alert_client
                .try_get_active_alerts_for_contract(&stranger, &target)
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

    // 17b. set_watcher_registry probes the target and rejects a contract that
    // doesn't implement the WatcherRegistry interface (#44)
    #[test]
    fn test_set_watcher_registry_rejects_invalid_contract() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        // A real, deployed contract — but not a WatcherRegistry, so it has
        // no `is_watcher_authorized` entry point for the probe to find.
        let not_a_watcher_registry = env.register(AlertRegistry, ());

        assert_eq!(
            client
                .try_set_watcher_registry(&admin, &not_a_watcher_registry)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidWatcherRegistry
        );
        // The rejected configuration must not have been persisted.
        assert!(client.get_watcher_registry().is_none());
        assert!(!client.is_watcher_gating_enabled());
    }

    // 17c. set_watcher_registry rejects a plain (non-contract) address (#44)
    #[test]
    fn test_set_watcher_registry_rejects_non_contract_address() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let not_a_contract = Address::generate(&env);

        assert_eq!(
            client
                .try_set_watcher_registry(&admin, &not_a_contract)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidWatcherRegistry
        );
    }

    // 17d. set_watcher_registry accepts a real WatcherRegistry after a prior
    // misconfigured attempt was rejected (#44)
    #[test]
    #[cfg(feature = "testutils")]
    fn test_set_watcher_registry_recovers_after_invalid_attempt() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();
        let admin = Address::generate(&env);
        alert_client.initialize(&admin);

        let bogus = env.register(AlertRegistry, ());
        assert_eq!(
            alert_client
                .try_set_watcher_registry(&admin, &bogus)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidWatcherRegistry
        );
        assert!(alert_client.get_watcher_registry().is_none());

        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);
        assert_eq!(
            alert_client.get_watcher_registry().unwrap(),
            watcher_contract_id
    // 17b. clear_watcher_registry disables gating; set_watcher_registry can
    // re-enable it afterward.
    #[test]
    #[cfg(feature = "testutils")]
    fn test_clear_watcher_registry_disables_then_reconfigure() {
        let (env, alert_client, watcher_client) = setup_with_watcher_registry();

        let admin = Address::generate(&env);
        let watcher = Address::generate(&env);
        let stranger = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        watcher_client.initialize(&admin);
        watcher_client.register_watcher(&admin, &watcher);

        alert_client.initialize(&admin);
        let watcher_contract_id = watcher_client.address.clone();
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);
        assert!(alert_client.is_watcher_gating_enabled());

        alert_client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        // Gating active: unregistered querier is rejected.
        assert_eq!(
            alert_client
                .try_get_alerts_for_contract(&stranger, &target)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );

        // Clear gating.
        alert_client.clear_watcher_registry(&admin);
        assert!(alert_client.get_watcher_registry().is_none());
        assert!(!alert_client.is_watcher_gating_enabled());

        // Any querier can now read.
        assert_eq!(
            alert_client
                .get_alerts_for_contract(&stranger, &target)
                .len(),
            1
        );

        // Re-configure gating.
        alert_client.set_watcher_registry(&admin, &watcher_contract_id);
        assert!(alert_client.is_watcher_gating_enabled());
        assert_eq!(
            alert_client
                .try_get_alerts_for_contract(&stranger, &target)
                .unwrap_err()
                .unwrap(),
            ContractError::NotAWatcher
        );
        assert_eq!(
            alert_client
                .get_alerts_for_contract(&watcher, &target)
                .len(),
            1
        );
    }

    // 17c. clear_watcher_registry rejects non-admin callers.
    #[test]
    fn test_clear_watcher_registry_non_admin_rejected() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        client.initialize(&admin);

        assert_eq!(
            client
                .try_clear_watcher_registry(&attacker)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // 17d. clear_watcher_registry requires the contract to be initialized.
    #[test]
    fn test_clear_watcher_registry_not_initialized() {
        let (env, client) = setup();
        let admin = Address::generate(&env);

        assert_eq!(
            client
                .try_clear_watcher_registry(&admin)
                .unwrap_err()
                .unwrap(),
            ContractError::NotInitialized
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

        let before = client.get_alert(&owner, &id).unwrap();
        assert_eq!(
            before.created_at, before.updated_at,
            "created_at and updated_at should be equal right after registration"
        );

        // Advance the ledger clock by 1 second so the update lands at T+1.
        env.ledger().with_mut(|li| {
            li.timestamp += 1;
        });

        client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true);

        let after = client.get_alert(&owner, &id).unwrap();
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

        let cfg = client.get_alert(&owner, &id).unwrap();
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

        let cfg = client.get_alert(&owner, &id).unwrap();
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

        let active = client.get_active_alerts_for_contract(&owner, &target);
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

        let active = client.get_active_alerts_for_contract(&owner, &target);
        assert_eq!(active.len(), 0);
    }

    // 25. get_active_alerts_for_contract — returns empty for unknown contract
    #[test]
    fn test_get_active_alerts_for_contract_empty() {
        let (env, client) = setup();
        let target = Address::generate(&env);
        assert_eq!(
            client
                .get_active_alerts_for_contract(&Address::generate(&env), &target)
                .len(),
            0
        );
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

        let active = client.get_active_alerts_for_contract(&owner, &target);
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

    // ── Two-step admin transfer (#196) ───────────────────────────────────────

    // Happy path: propose → accept hands control to new_admin
    #[test]
    fn test_propose_accept_admin_transfer_happy_path() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        assert_eq!(
            client
                .try_propose_admin_transfer(&admin, &new_admin)
                .unwrap(),
            Ok(())
        );
        // old admin retains control before acceptance
        assert_eq!(client.get_admin(), admin);

        assert_eq!(
            client.try_accept_admin_transfer(&new_admin).unwrap(),
            Ok(())
        );
        assert_eq!(client.get_admin(), new_admin);

        // new_admin can exercise admin privileges
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );
        assert_eq!(
            client.try_remove_alert_by_admin(&new_admin, &id).unwrap(),
            Ok(())
        );
    }

    // old admin loses privileges once the transfer is accepted
    #[test]
    fn test_old_admin_rejected_after_two_step_transfer() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);
        client.accept_admin_transfer(&new_admin);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        );
        assert_eq!(
            client
                .try_remove_alert_by_admin(&admin, &id)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // accept by the wrong address is rejected with NoPendingTransfer
    #[test]
    fn test_accept_admin_transfer_wrong_address() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        let attacker = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);

        assert_eq!(
            client
                .try_accept_admin_transfer(&attacker)
                .unwrap_err()
                .unwrap(),
            ContractError::NoPendingTransfer
        );
        // admin is unchanged
        assert_eq!(client.get_admin(), admin);
    }

    // accept with no pending proposal is rejected with NoPendingTransfer
    #[test]
    fn test_accept_admin_transfer_none_pending() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        assert_eq!(
            client
                .try_accept_admin_transfer(&new_admin)
                .unwrap_err()
                .unwrap(),
            ContractError::NoPendingTransfer
        );
    }

    // cancel clears the proposal; subsequent accept is rejected
    #[test]
    fn test_cancel_admin_transfer() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);
        assert_eq!(
            client.try_cancel_admin_transfer(&admin).unwrap(),
            Ok(())
        );

        // the cancelled proposal can no longer be accepted
        assert_eq!(
            client
                .try_accept_admin_transfer(&new_admin)
                .unwrap_err()
                .unwrap(),
            ContractError::NoPendingTransfer
        );
        // admin is unchanged
        assert_eq!(client.get_admin(), admin);
    }

    // cancel with no pending proposal is rejected with NoPendingTransfer
    #[test]
    fn test_cancel_admin_transfer_none_pending() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        assert_eq!(
            client
                .try_cancel_admin_transfer(&admin)
                .unwrap_err()
                .unwrap(),
            ContractError::NoPendingTransfer
        );
    }

    // proposing again overwrites the previous pending address
    #[test]
    fn test_propose_admin_transfer_overwrite() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let first_candidate = Address::generate(&env);
        let second_candidate = Address::generate(&env);

        client.propose_admin_transfer(&admin, &first_candidate);
        // overwrite with a different address
        assert_eq!(
            client
                .try_propose_admin_transfer(&admin, &second_candidate)
                .unwrap(),
            Ok(())
        );

        // the first candidate can no longer accept
        assert_eq!(
            client
                .try_accept_admin_transfer(&first_candidate)
                .unwrap_err()
                .unwrap(),
            ContractError::NoPendingTransfer
        );
        // the second candidate can accept
        assert_eq!(
            client
                .try_accept_admin_transfer(&second_candidate)
                .unwrap(),
            Ok(())
        );
        assert_eq!(client.get_admin(), second_candidate);
    }

    // non-admin cannot propose a transfer
    #[test]
    fn test_propose_admin_transfer_unauthorized() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let attacker = Address::generate(&env);
        let new_admin = Address::generate(&env);

        assert_eq!(
            client
                .try_propose_admin_transfer(&attacker, &new_admin)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // non-admin cannot cancel a pending transfer
    #[test]
    fn test_cancel_admin_transfer_unauthorized() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);
        let attacker = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);

        assert_eq!(
            client
                .try_cancel_admin_transfer(&attacker)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }

    // propose emits admin.propose event; accept emits admin.transfer event
    #[test]
    fn test_propose_and_accept_admin_transfer_events() {
        use soroban_sdk::{symbol_short, testutils::Events as _, FromVal, Symbol};

        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);

        let events = env.events().all();
        let propose_event = events.iter().find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("admin")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("propose")
        });
        assert!(propose_event.is_some(), "admin.propose event must be emitted");

        client.accept_admin_transfer(&new_admin);

        let events = env.events().all();
        let transfer_event = events.iter().find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("admin")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("transfer")
        });
        assert!(
            transfer_event.is_some(),
            "admin.transfer event must be emitted"
        );

        let (_, _, data) = transfer_event.unwrap();
        let emitted_new_admin: Address = FromVal::from_val(&env, &data);
        assert_eq!(emitted_new_admin, new_admin);
    }

    // cancel emits admin.cancel event
    #[test]
    fn test_cancel_admin_transfer_emits_event() {
        use soroban_sdk::{symbol_short, testutils::Events as _, Symbol};

        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let new_admin = Address::generate(&env);

        client.propose_admin_transfer(&admin, &new_admin);
        client.cancel_admin_transfer(&admin);

        let events = env.events().all();
        let cancel_event = events.iter().find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("admin")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("cancel")
        });
        assert!(cancel_event.is_some(), "admin.cancel event must be emitted");
    }

    // auth-failure: propose_admin_transfer requires admin's auth signature
    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_propose_admin_transfer_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        env.set_auths(&[]);
        client.propose_admin_transfer(&admin, &new_admin);
    }

    // auth-failure: accept_admin_transfer requires new_admin's auth signature
    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_accept_admin_transfer_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        client.propose_admin_transfer(&admin, &new_admin);
        env.set_auths(&[]);
        client.accept_admin_transfer(&new_admin);
    }

    // auth-failure: cancel_admin_transfer requires admin's auth signature
    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_cancel_admin_transfer_requires_auth() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin);
        client.propose_admin_transfer(&admin, &new_admin);
        env.set_auths(&[]);
        client.cancel_admin_transfer(&admin);
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

        let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
        assert_eq!(results.len(), 2);
    }

    // 19. Returns empty vec when no alerts exist
    #[test]
    fn test_get_alerts_modified_since_empty_registry() {
        let (_env, client) = setup();
        let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
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
        let results = client.get_alerts_modified_since(&1000u64, &0u32, &u32::MAX);
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
        let results = client.get_alerts_modified_since(&500u64, &0u32, &u32::MAX);
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
        let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
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
        let results = client.get_alerts_modified_since(&42u64, &0u32, &u32::MAX);
        assert_eq!(results.len(), 1);

        // since == updated_at + 1 should exclude it
        let results_after = client.get_alerts_modified_since(&43u64, &0u32, &u32::MAX);
        assert_eq!(results_after.len(), 0);
    }

    // 24. get_alerts_modified_since_ledger returns all alerts when since_ledger == 0
    #[test]
    fn test_get_alerts_modified_since_ledger_zero_returns_all() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

        let results = client.get_alerts_modified_since_ledger(&0u32, &0u32, &u32::MAX);
        assert_eq!(results.len(), 2);
    }

    // 25. get_alerts_modified_since_ledger returns empty vec on empty registry
    #[test]
    fn test_get_alerts_modified_since_ledger_empty_registry() {
        let (_env, client) = setup();
        let results = client.get_alerts_modified_since_ledger(&0u32, &0u32, &u32::MAX);
        assert_eq!(results.len(), 0);
    }

    // 26. Filters out alerts whose updated_ledger is before since_ledger (unambiguous multi-ledger sync)
    #[test]
    fn test_get_alerts_modified_since_ledger_filters_by_sequence() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        // Multiple ledgers in the same close-time second (timestamp 1000)
        env.ledger().with_mut(|li| {
            li.timestamp = 1000;
            li.sequence_number = 50;
        });
        client.register_alert(&owner, &target, &str(&env, "L50"), &hash64(&env), &vec![&env]);

        env.ledger().with_mut(|li| {
            li.timestamp = 1000;
            li.sequence_number = 51;
        });
        client.register_alert(&owner, &target, &str(&env, "L51"), &hash64(&env), &vec![&env]);

        // Querying with since_ledger = 51 returns only the second alert
        let results = client.get_alerts_modified_since_ledger(&51u32, &0u32, &u32::MAX);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).unwrap().label, str(&env, "L51"));
    }

    // 27. since_ledger boundary value exactly equal is included; +1 excludes it
    #[test]
    fn test_get_alerts_modified_since_ledger_boundary_inclusive() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        env.ledger().with_mut(|li| li.sequence_number = 75);
        client.register_alert(
            &owner,
            &target,
            &str(&env, "BoundarySeq"),
            &hash64(&env),
            &vec![&env],
        );

        let results = client.get_alerts_modified_since_ledger(&75u32, &0u32, &u32::MAX);
        assert_eq!(results.len(), 1);

        let results_after = client.get_alerts_modified_since_ledger(&76u32, &0u32, &u32::MAX);
        assert_eq!(results_after.len(), 0);
    }

    // ── Auth-failure tests (no mock_all_auths) ────────────────────────────────

    // #195 — initialize must require a valid signature from the admin address so
    // that no one can front-run the initialization window after deployment and
    // claim the admin role without owning the corresponding key.
    #[test]
    #[should_panic(expected = "Error(Auth, InvalidAction)")]
    fn test_initialize_requires_auth() {
        let env = Env::default();
        // No mock_all_auths — any require_auth() call will fail.
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        // Calling initialize without a valid signature must panic with
        // Error(Auth, InvalidAction), not succeed and hand over admin control.
        client.initialize(&admin);
    }

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
        let modified = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
        let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
        let scan_cost = cpu_after.saturating_sub(cpu_before);

        assert_eq!(modified.len() as usize, N);
        // Assert an upper bound regression guard on the scan cost for N=50
        assert!(
            scan_cost < 15_000_000,
            "get_alerts_modified_since cost {scan_cost} exceeded upper bound 15M instructions"
        );
    }

    /// Confirms the fix for #38: a caller that pages with a small, bounded
    /// `limit` pays a scan cost proportional to that `limit`, not to the
    /// total number of alerts ever registered. This is what stops an
    /// attacker from inflating `NEXT_ID` (via repeated `register_alert`
    /// calls) from degrading the read path for every other caller — each
    /// caller controls their own scan cost via `limit`.
    #[test]
    fn test_get_alerts_modified_since_pagination_bounds_scan_cost() {
        const N: u32 = 400;
        const PAGE: u32 = 10;

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let hash = hash64(&env);
        let rules = vec![&env, str(&env, "rule:transfer")];

        for _ in 0..N {
            let label = str(&env, "Alert");
            client.register_alert(&owner, &target, &label, &hash, &rules);
        }
        assert_eq!(client.get_alert_count(), u64::from(N));

        // A small page near the end of a large registry must still be cheap:
        // cost is bounded by PAGE, not by N.
        let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
        let page = client.get_alerts_modified_since(&0u64, &390u32, &PAGE);
        let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
        let page_scan_cost = cpu_after.saturating_sub(cpu_before);

        assert_eq!(page.len() as usize, PAGE as usize);
        // A page of 10 out of a 400-alert registry should cost nowhere near
        // the ~15M-instruction ceiling asserted for a full 50-alert scan
        // above — if this ever regresses to an O(N) scan the cost will blow
        // well past this bound.
        assert!(
            page_scan_cost < 2_000_000,
            "paginated get_alerts_modified_since cost {page_scan_cost} exceeded upper bound 2M instructions for a page of {PAGE}"
        );

        // Requesting past the end of the registry returns an empty page
        // rather than scanning anything.
        let empty_page = client.get_alerts_modified_since(&0u64, &N, &PAGE);
        assert_eq!(empty_page.len(), 0);
    }

    /// Load test quantifying the repeated rescan cost in `assert_per_owner_limit` (#39).
    /// Registers alerts with an active per-owner limit and benchmarks instruction growth,
    /// asserting an upper bound regression guard.
    /// Load test quantifying the (formerly O(n²)) cost of `assert_per_owner_limit` (#39).
    ///
    /// Registers `LIMIT` alerts for the same owner under an active per-owner
    /// limit and benchmarks instruction growth across the run. Before the
    /// fix, `assert_per_owner_limit` rescanned `get_active_alert_count` (an
    /// O(n) full-index scan) on every call, so `last_reg_cost` grew roughly
    /// linearly with `LIMIT` — at LIMIT=100 the last call cost ~100x the
    /// first. With the running per-owner counter, the limit check is O(1),
    /// so cost per registration should stay flat regardless of `LIMIT`.
    #[test]
    fn test_load_assert_per_owner_limit_instruction_cost() {
        const LIMIT: u32 = 100;

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
        // Before/after regression guard: with an O(1) per-owner counter, the
        // Nth registration should not cost meaningfully more than the 1st.
        // (Under the old O(n) rescan, this ratio grew with LIMIT itself.)
        assert!(
            last_reg_cost < first_reg_cost.saturating_mul(3),
            "registration cost grew from {first_reg_cost} to {last_reg_cost} across {LIMIT} \
             calls — assert_per_owner_limit is no longer O(1)"
        );
        // Assert an upper bound regression guard on total batch registration cost with limit checks
        assert!(
            total_registration_cost < 50_000_000,
            "Total registration cost {total_registration_cost} exceeded upper bound 50M instructions"
        );
    }

    /// `get_non_removed_alert_count` is O(1) regardless of how many alerts an
    /// owner has ever registered — it reads a maintained counter instead of
    /// rescanning `OwnerIndex` (#39).
    #[test]
    fn test_get_non_removed_alert_count_instruction_cost_is_constant() {
        const N: u32 = 200;

        let (env, client) = setup();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let hash = hash64(&env);
        let rules = vec![&env];

        for _ in 0..N {
            client.register_alert(&owner, &target, &str(&env, "Alert"), &hash, &rules);
        }

        let before = env.cost_estimate().budget().cpu_instruction_cost();
        let count = client.get_non_removed_alert_count(&owner);
        let after = env.cost_estimate().budget().cpu_instruction_cost();
        let cost = after.saturating_sub(before);

        assert_eq!(count, N);
        // An O(n) rescan at N=200 would cost far more than a single storage
        // read; this bound would fail under a scan-based implementation.
        assert!(
            cost < 200_000,
            "get_non_removed_alert_count cost {cost} at N={N} looks O(n), not O(1)"
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
        let cfg = client.get_alert(&owner, &id).unwrap();
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
        let (env, client) = setup();
        assert!(client
            .get_alert_active(&Address::generate(&env), &999u64)
            .is_none());
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

        assert_eq!(client.get_alert_active(&owner, &id), Some(true));
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

        assert_eq!(client.get_alert_active(&owner, &id), Some(true));

        client.update_alert(&owner, &id, &vec![&env], &false);
        assert_eq!(client.get_alert_active(&owner, &id), Some(false));

        client.update_alert(&owner, &id, &vec![&env], &true);
        assert_eq!(client.get_alert_active(&owner, &id), Some(true));
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

        assert_eq!(client.get_alert_active(&owner, &id), Some(true));
        client.remove_alert(&owner, &id);
        assert!(client.get_alert_active(&owner, &id).is_none());
    }

    // 21a. get_alert_owner returns the owner after registration
    #[test]
    fn test_get_alert_owner_after_register() {
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
            client.get_alert_owner(&owner, &id).unwrap(),
            Some(owner)
        );
    }

    // 21b. get_alert_owner reflects an accepted ownership transfer
    #[test]
    fn test_get_alert_owner_after_transfer() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );

        client.propose_alert_transfer(&owner, &id, &new_owner);

        client.accept_alert_transfer(&new_owner, &id);
        assert_eq!(
            client.get_alert_owner(&owner, &id).unwrap(),
            Some(new_owner)
        );
    }

    // 21c. get_alert_owner returns None for nonexistent ID
    #[test]
    fn test_get_alert_owner_nonexistent() {
        let (env, client) = setup();
        assert!(client
            .get_alert_owner(&Address::generate(&env), &999u64)
            .is_none());
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

        assert_eq!(client.get_alert_active(&owner, &id1), Some(true));
        assert_eq!(client.get_alert_active(&owner, &id2), Some(true));
        assert_eq!(client.get_alert_active(&owner, &id3), Some(true));

        let count = client.deactivate_all_alerts(&owner);
        assert_eq!(count, 3);

        assert_eq!(client.get_alert_active(&owner, &id1), Some(false));
        assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
        assert_eq!(client.get_alert_active(&owner, &id3), Some(false));
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

        assert_eq!(client.get_alert_active(&Address::generate(&env), &id1), Some(false));
        assert_eq!(client.get_alert_active(&Address::generate(&env), &id2), Some(true));
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
        assert!(client.get_alert(&owner, &id1).is_none());
        // id2 is now inactive
        assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
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
    // (Result-returning contract functions still panic via the plain client
    // call when they return Err — this mirrors WatcherRegistry::get_admin.)
    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_get_admin_not_initialized() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);
        client.get_admin();
    }

    // 18b. get_admin returns a typed NotInitialized error via try_get_admin (#41)
    #[test]
    fn test_try_get_admin_uninitialized() {
        let env = Env::default();
        let contract_id = env.register(AlertRegistry, ());
        let client = AlertRegistryClient::new(&env, &contract_id);

        assert_eq!(
            client.try_get_admin().unwrap_err().unwrap(),
            ContractError::NotInitialized
        );
    }

    // 18c. get_admin returns Ok(admin) once initialized
    #[test]
    fn test_try_get_admin_after_initialize() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        assert_eq!(client.try_get_admin().unwrap().unwrap(), admin);
        assert_eq!(client.get_admin(), admin);
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
        let cfg = client.get_alert(&owner, &id).unwrap();
        assert!(!cfg.active);

        // reactivate
        assert_eq!(
            client
                .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true)
                .unwrap(),
            Ok(())
        );
        let cfg = client.get_alert(&owner, &id).unwrap();
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

        let original_updated_at = client.get_alert(&owner, &id).unwrap().updated_at;
        env.ledger().set_timestamp(original_updated_at + 100);

        client
            .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap()
            .unwrap();

        let cfg = client.get_alert(&owner, &id).unwrap();
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

        let before = client.get_alert(&owner, &id).unwrap();
        client.bump_alert(&id, &17_280u32);
        let after = client.get_alert(&owner, &id).unwrap();

        // All fields must be identical after a bump
        assert_eq!(after.label, before.label);
        assert_eq!(after.webhook_hash, before.webhook_hash);
        assert_eq!(after.rules.len(), before.rules.len());
        assert_eq!(after.owner, before.owner);
        assert_eq!(after.target_contract, before.target_contract);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.updated_ledger, before.updated_ledger);
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
