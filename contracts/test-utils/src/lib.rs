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

/// Set up an [`AlertRegistry`] environment.
///
/// The contract is deployed through its constructor, so the returned client is
/// already initialised with the returned admin.
///
/// Returns `(env, client, admin)`.
pub fn setup_alert_registry() -> (Env, AlertRegistryClient<'static>, Address) {
    use soroban_sdk::testutils::Address as _;
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(AlertRegistry, (admin.clone(),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client, admin)
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
/// Returns `(env, alert_client, watcher_client, admin)`; both contracts are
/// deployed through their constructors with `admin`.
pub fn setup_both() -> (
    Env,
    AlertRegistryClient<'static>,
    WatcherRegistryClient<'static>,
    Address,
) {
    use soroban_sdk::testutils::Address as _;
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let alert_id = env.register(AlertRegistry, (admin.clone(),));
    let watcher_id = env.register(WatcherRegistry, (admin.clone(),));
    let alert_client = AlertRegistryClient::new(&env, &alert_id);
    let watcher_client = WatcherRegistryClient::new(&env, &watcher_id);
    (env, alert_client, watcher_client, admin)
}
