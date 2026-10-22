use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Env, FromVal,
};

fn setup() -> (Env, Address, WatcherRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(WatcherRegistry, (&admin,));
    let client = WatcherRegistryClient::new(&env, &contract_id);
    (env, admin, client)
}

/// Regression test for historical bug:
/// `replace_watcher could drop the replacement watcher.`
///
/// Ensures that when replacing `old_watcher` with `new_watcher`, the `new_watcher`
/// is successfully added and authorized, rather than being dropped from the registry.
#[test]
fn test_regression_replace_watcher_dropping_replacement() {
    let (env, admin, client) = setup();
    let old_watcher = Address::generate(&env);
    let new_watcher = Address::generate(&env);

    client.register_watcher(&admin, &old_watcher);
    assert!(client.is_watcher_authorized(&old_watcher));
    assert!(!client.is_watcher_authorized(&new_watcher));

    // Replace old with new
    assert_eq!(
        client
            .try_replace_watcher(&admin, &old_watcher, &new_watcher)
            .unwrap(),
        Ok(())
    );

    // Verify old is removed and replacement is NOT dropped
    assert!(!client.is_watcher_authorized(&old_watcher));
    assert!(client.is_watcher_authorized(&new_watcher));

    let list = client.get_watchers();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), new_watcher);
    assert_eq!(client.get_watcher_count(), 1);
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

    client.remove_watcher(&admin, &w3);
    assert_eq!(client.get_watcher_count(), 0);
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

    let events = env.events().all();
    assert_eq!(
        events.events().len(),
        0,
        "No event should be emitted when removing an unregistered watcher"
    );
}

/// Regression test for historical bug:
/// `clear_all_watchers emits one event per removed watcher.`
///
/// Ensures that clearing all watchers emits individual `("watcher", "remove")`
/// events for all removed addresses so off-chain systems immediately revoke trust.
#[test]
fn test_regression_clear_all_watchers_emits_one_event_per_removed_watcher() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);

    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);
    assert_eq!(client.get_watcher_count(), 2);

    client.clear_all_watchers(&admin);

    let events = env.events().all();
    let events = events.events();
    // Should have emitted 2 remove events (one for each removed watcher)
    assert_eq!(events.len(), 2);
    for event in events.iter() {
        let v0 = match &event.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0,
        };
        assert_eq!(v0.topics.len(), 2);
        let emitted_addr: Address = soroban_sdk::TryFromVal::try_from_val(&env, &v0.data)
            .expect("event data decodes as an address");
        assert!(emitted_addr == w1 || emitted_addr == w2);
    let mut removed = 0;
    for i in 0..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        if topics.len() == 2
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("watcher")
            && soroban_sdk::Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("remove")
        {
            let (emitted_addr,): (Address,) = soroban_sdk::FromVal::from_val(&env, &data);
            assert!(emitted_addr == w1 || emitted_addr == w2);
            removed += 1;
        }
    }
    assert_eq!(removed, 2);

    assert_eq!(client.get_watcher_count(), 0);
    assert_eq!(client.get_watchers().len(), 0);
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

/// Regression test for #380:
/// Redundant `pause` (when already paused) and `unpause` (when already running)
/// calls are silent no-ops that do not emit duplicate `admin.pause` or
/// `admin.unpause` events.
#[test]
fn test_regression_pause_and_unpause_redundant_transitions_are_noops() {
    let (env, admin, client) = setup();

    client.unpause(&admin);
    assert!(!client.is_paused());
    assert!(env.events().all().is_empty());

    client.pause(&admin);
    assert!(client.is_paused());
    assert_eq!(env.events().all().len(), 1);

    client.pause(&admin);
    assert!(client.is_paused());
    assert!(env.events().all().is_empty());

    client.unpause(&admin);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), 1);

    client.unpause(&admin);
    assert!(!client.is_paused());
    assert!(env.events().all().is_empty());
}

/// Regression test for #370: an admin transfer proposal cannot be accepted
/// after `ADMIN_TRANSFER_EXPIRY_LEDGERS`.
#[test]
fn test_regression_admin_transfer_proposal_expires() {
    use soroban_sdk::testutils::Ledger as _;

    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);

    // One ledger past the expiry ledger.
    env.ledger()
        .with_mut(|li| li.sequence_number += ADMIN_TRANSFER_EXPIRY_LEDGERS + 1);
    assert_eq!(
        client
            .try_accept_admin_transfer(&new_admin)
            .unwrap_err()
            .unwrap(),
        ContractError::AdminTransferExpired
    );

    // A fresh proposal replaces the expired one and can be accepted.
    client.propose_admin_transfer(&admin, &new_admin);
    client.accept_admin_transfer(&new_admin);
    assert!(client.is_admin(&new_admin));
}

/// Regression test for #415: proposing a transfer to an address that is
/// already the sole current admin must be rejected rather than creating a
/// meaningless pending transfer and emitting an ("admin", "propose") event.
#[test]
fn test_regression_propose_admin_transfer_rejects_sole_admin_as_new_admin() {
    let (env, admin, client) = setup();

    let res = client.try_propose_admin_transfer(&admin, &admin);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::InvalidTransferRecipient,
        "Proposing the sole admin as new_admin must be rejected"
    );
    // No pending transfer should have been created, and no event emitted.
    assert!(env.events().all().is_empty());
    assert_eq!(
        client.try_accept_admin_transfer(&admin).unwrap_err().unwrap(),
        ContractError::NoPendingTransfer
    );

    // A transfer to a genuinely different address still works.
    let other = Address::generate(&env);
    client.propose_admin_transfer(&admin, &other);
    client.accept_admin_transfer(&other);
    assert!(client.is_admin(&other));
}

/// Test for #416: optional per-watcher metadata (label/operator) can be set,
/// read back, and cleared by an admin.
#[test]
fn test_watcher_info_set_get_and_clear() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    assert!(client.get_watcher_info(&watcher).is_none());

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper-east"),
        operator: String::from_str(&env, "Acme Monitoring"),
    };
    client.set_watcher_info(&admin, &watcher, &Some(info.clone()));

    let stored = client.get_watcher_info(&watcher);
    assert_eq!(stored, Some(info));

    // Clearing with None removes the stored metadata.
    client.set_watcher_info(&admin, &watcher, &None);
    assert!(client.get_watcher_info(&watcher).is_none());

    // Non-admins cannot set watcher info.
    let not_admin = Address::generate(&env);
    let res = client.try_set_watcher_info(
        &not_admin,
        &watcher,
        &Some(WatcherInfo {
            label: String::from_str(&env, "x"),
            operator: String::from_str(&env, "y"),
        }),
    );
    assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
}

/// Test for #417: an unscoped watcher is authorized for every target, while
/// a watcher with a configured scope is only authorized for listed targets.
#[test]
fn test_watcher_scope_restricts_is_watcher_authorized_for() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    // Default: unscoped, authorized for any target.
    assert!(client.is_watcher_authorized_for(&watcher, &target_a));
    assert!(client.is_watcher_authorized_for(&watcher, &target_b));
    assert!(client.get_watcher_scope(&watcher).is_empty());

    // Restrict to target_a only.
    let scope = soroban_sdk::vec![&env, target_a.clone()];
    client.set_watcher_scope(&admin, &watcher, &scope);
    assert!(client.is_watcher_authorized_for(&watcher, &target_a));
    assert!(!client.is_watcher_authorized_for(&watcher, &target_b));
    assert_eq!(client.get_watcher_scope(&watcher), scope);

    // A watcher that isn't authorized at all is never authorized for a target,
    // regardless of scope.
    let unregistered = Address::generate(&env);
    assert!(!client.is_watcher_authorized_for(&unregistered, &target_a));

    // Clearing the scope (empty vec) restores unrestricted access.
    client.set_watcher_scope(&admin, &watcher, &soroban_sdk::vec![&env]);
    assert!(client.is_watcher_authorized_for(&watcher, &target_b));
}

/// Test for #418: `are_watchers_authorized` reports per-address authorization
/// in one call and is capped at MAX_PAGE_SIZE entries.
#[test]
fn test_are_watchers_authorized_batch_query() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let unregistered = Address::generate(&env);
    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);

    let batch = soroban_sdk::vec![&env, w1.clone(), unregistered.clone(), w2.clone()];
    let result = client.are_watchers_authorized(&batch);
    assert_eq!(result.len(), 3);
    assert!(result.get(0).unwrap());
    assert!(!result.get(1).unwrap());
    assert!(result.get(2).unwrap());

    // Empty input yields empty output.
    let empty: Vec<Address> = soroban_sdk::vec![&env];
    assert_eq!(client.are_watchers_authorized(&empty).len(), 0);

    // Entries beyond MAX_PAGE_SIZE are ignored rather than reported false.
    let mut oversized: Vec<Address> = soroban_sdk::vec![&env];
    for _ in 0..(MAX_PAGE_SIZE + 5) {
        oversized.push_back(w1.clone());
    }
    assert_eq!(
        client.are_watchers_authorized(&oversized).len() as u32,
        MAX_PAGE_SIZE
    );
}
