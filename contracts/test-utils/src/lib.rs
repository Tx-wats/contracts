//! Shared test helpers for alert-registry and watcher-registry
//! integration tests.
//!
//! Add to `[dev-dependencies]` in any workspace member that needs it:
//!
//! ```toml
//! test-utils = { path = "../test-utils" }
//! ```

use alert_registry::{AlertRegistry, AlertRegistryClient};
use soroban_sdk::{Address, BytesN, Env, String};
use watcher_registry::{WatcherRegistry, WatcherRegistryClient};

// ── String helpers ────────────────────────────────────────────────────────────

/// Wrap a `&str` literal into a Soroban [`String`].
pub fn str(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

/// A webhook hash (32-byte SHA-256 digest) with every byte set to `c`.
///
/// `register_alert`, `update_webhook` and `propose_webhook` take the webhook
/// hash as `BytesN<32>`; vary `c` when a test needs two hashes that must
/// differ.
pub fn hash64c(env: &Env, c: char) -> BytesN<32> {
    BytesN::from_array(env, &[c as u8; 32])
}

/// The default webhook hash used by tests.
pub fn hash64(env: &Env) -> BytesN<32> {
    hash64c(env, '0')
}

/// Build a Soroban [`String`] consisting of `n` repetitions of the ASCII
/// character `ch`.  Handy for boundary-length tests.
pub fn str_repeat(env: &Env, ch: char, n: usize) -> String {
    let s = std::iter::repeat_n(ch, n).collect::<std::string::String>();
    String::from_str(env, &s)
}

// ── Setup helpers ─────────────────────────────────────────────────────────────

/// Set up a bare [`AlertRegistry`] environment (no admin initialised).
///
/// Returns `(env, client)`.
pub fn setup_alert_registry() -> (Env, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client)
}

/// Set up a [`WatcherRegistry`] environment with the admin already initialised.
///
/// Returns `(env, admin, client)`.
pub fn setup_watcher_registry() -> (Env, Address, WatcherRegistryClient<'static>) {
    use soroban_sdk::testutils::Address as _;
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(WatcherRegistry, (&admin,));
    let client = WatcherRegistryClient::new(&env, &contract_id);
    (env, admin, client)
}

/// Set up both registries in a single shared environment — suitable for
/// cross-contract / integration tests.
///
/// Returns `(env, alert_client, watcher_client)`.
pub fn setup_both() -> (
    Env,
    AlertRegistryClient<'static>,
    WatcherRegistryClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let alert_id = env.register(AlertRegistry, ());
    let watcher_id = env.register(WatcherRegistry, ());
    let alert_client = AlertRegistryClient::new(&env, &alert_id);
    let watcher_client = WatcherRegistryClient::new(&env, &watcher_id);
    (env, alert_client, watcher_client)
}

/// A fixture for gated integration tests where AlertRegistry is configured with
/// a WatcherRegistry as its gating mechanism.
pub struct GatedFixture<'a> {
    pub env: Env,
    pub admin: Address,
    pub watcher: Address,
    pub alert_client: AlertRegistryClient<'a>,
    pub watcher_client: WatcherRegistryClient<'a>,
}

/// Set up both registries with complete gating configuration — suitable for
/// integration tests that require a watcher-gated AlertRegistry.
///
/// Performs the following setup:
/// 1. Creates both contract instances
/// 2. Initializes WatcherRegistry with an admin
/// 3. Registers a watcher in WatcherRegistry
/// 4. Registers the WatcherRegistry as the gating mechanism in AlertRegistry
///
/// Returns a [`GatedFixture`] containing the env, addresses, and both clients.
pub fn setup_gated() -> GatedFixture<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let alert_id = env.register(AlertRegistry, ());
    let watcher_id = env.register(WatcherRegistry, ());
    let alert_client = AlertRegistryClient::new(&env, &alert_id);
    let watcher_client = WatcherRegistryClient::new(&env, &watcher_id);

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);

    // Initialize WatcherRegistry with the admin
    watcher_client.initialize(&admin);

    // Register the watcher in WatcherRegistry
    watcher_client.register_watcher(&admin, &watcher);

    // Set up gating: point AlertRegistry to WatcherRegistry for access control
    alert_client.set_watcher_registry(&admin, &watcher_id);

    GatedFixture {
        env,
        admin,
        watcher,
        alert_client,
        watcher_client,
    }
}

// ── Ledger advancement helpers ────────────────────────────────────────────────

/// Advance the ledger by `n` sequence numbers to simulate ledger progression
/// and observe TTL expiry behavior. This is essential for testing entry
/// archival scenarios where alerts, indices, counters, or instance entries
/// approach or exceed their DEFAULT_TTL.
pub fn advance_ledger(env: &Env, n: u32) {
    let current = env.ledger().sequence();
    env.ledger().set_sequence_number(current + n);
}
