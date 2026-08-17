//! Shared test helpers for alert-registry and watcher-registry
//! integration tests.
//!
//! Add to `[dev-dependencies]` in any workspace member that needs it:
//!
//! ```toml
//! test-utils = { path = "../test-utils" }
//! ```

use alert_registry::{AlertRegistry, AlertRegistryClient};
use soroban_sdk::{Address, Env, String};
use watcher_registry::{WatcherRegistry, WatcherRegistryClient};

// ── String helpers ────────────────────────────────────────────────────────────

/// Wrap a `&str` literal into a Soroban [`String`].
pub fn str(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

/// A 64-character webhook hash of repeated `c`.
///
/// `register_alert`, `update_webhook` and `propose_webhook` all require a
/// webhook hash of exactly 64 characters; vary `c` when a test needs two
/// hashes that must differ.
pub fn hash64c(env: &Env, c: char) -> String {
    let buf = [c as u8; 64];
    String::from_str(env, core::str::from_utf8(&buf).unwrap())
}

/// The default valid 64-character webhook hash.
pub fn hash64(env: &Env) -> String {
    hash64c(env, '0')
}

/// Build a Soroban [`String`] consisting of `n` repetitions of the ASCII
/// character `ch`.  Handy for boundary-length tests.
pub fn str_repeat(env: &Env, ch: char, n: usize) -> String {
    let s = std::iter::repeat(ch)
        .take(n)
        .collect::<std::string::String>();
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
    let contract_id = env.register(WatcherRegistry, ());
    let client = WatcherRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
