use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, WatcherRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    // Deployed through the constructor (the only init path since soroban-sdk 25).
    let contract_id = env.register(WatcherRegistry, (admin.clone(),));
    let client = WatcherRegistryClient::new(&env, &contract_id);
    (env, admin, client)
}

/// Regression test for historical bug:
/// `WatcherRegistry::clear_all_watchers removed every watcher without emitting a
/// per-watcher event.`
///
/// With `MIN_WATCHERS = 1` in force the entrypoint refuses to empty a non-empty
/// registry at all (the per-watcher removal path is only reachable when
/// `MIN_WATCHERS` is 0), so this test now pins the guard: the call is rejected
/// and the registry is left exactly as it was.
#[test]
fn test_regression_clear_all_watchers_emits_one_event_per_removed_watcher() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);

    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);
    assert_eq!(client.get_watcher_count(), 2);

    assert_eq!(
        client.try_clear_all_watchers(&admin).unwrap_err().unwrap(),
        super::ContractError::BelowMinWatchers
    );

    // Nothing was cleared and no removal events were emitted.
    assert_eq!(client.get_watcher_count(), 2);
    assert_eq!(client.get_watchers().len(), 2);
    assert!(crate::emitted_events(&env).is_empty());
}

/// Regression test for historical bug:
/// `decrement_watcher_count never being called` / `get_watcher_count now decrements correctly on removal.`
///
/// Ensures that `remove_watcher` correctly decrements the active watcher count
/// instead of monotonically increasing or failing to decrement.
#[test]
fn test_regression_decrement_watcher_count_never_being_called() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);

    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);
    client.register_watcher(&admin, &w3);
    assert_eq!(client.get_watcher_count(), 3);

    client.remove_watcher(&admin, &w2);
    assert_eq!(client.get_watcher_count(), 2);

    client.remove_watcher(&admin, &w1);
    assert_eq!(client.get_watcher_count(), 1);

    // The last watcher cannot be removed (MIN_WATCHERS = 1).
    assert_eq!(
        client.try_remove_watcher(&admin, &w3).unwrap_err().unwrap(),
        ContractError::BelowMinWatchers
    );
    assert_eq!(client.get_watcher_count(), 1);
}

/// Regression test for historical bug:
/// `WatcherRegistry::remove_watcher no longer emits an event when the watcher address was not registered.`
///
/// Ensures that attempting to remove an unregistered address succeeds silently (no-op)
/// and does not emit any `("watcher", "remove")` event.
#[test]
fn test_regression_remove_watcher_unregistered_emits_no_event() {
    let (env, admin, client) = setup();
    let unregistered = Address::generate(&env);

    // Removing an unregistered watcher should be a silent no-op
    assert_eq!(
        client.try_remove_watcher(&admin, &unregistered).unwrap(),
        Ok(())
    );

    let events = crate::emitted_events(&env);
    assert_eq!(
        events.len(),
        0,
        "No event should be emitted when removing an unregistered watcher"
    );
}

/// Regression test for historical bug:
/// `remove_admin refusing to remove the last admin to prevent permanent lockout.`
///
/// Ensures that attempting to remove the sole admin returns `ContractError::LastAdmin`.
#[test]
fn test_regression_remove_last_admin_lockout() {
    let (_env, admin, client) = setup();

    let res = client.try_remove_admin(&admin, &admin);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::LastAdmin,
        "Removing the sole admin must return LastAdmin error"
    );
    assert_eq!(client.get_admins().len(), 1);
}
