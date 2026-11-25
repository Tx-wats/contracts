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

/// Regression test for #592: `clear_all_watchers` clears every watcher except
/// the kept set, emitting one `("watcher", "remove")` event per removed
/// watcher, and the kept watchers remain authorized.
#[test]
fn test_regression_clear_all_watchers_keeps_kept_set() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let kept = Address::generate(&env);

    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);
    client.register_watcher(&admin, &kept);
    assert_eq!(client.get_watcher_count(), 3);

    let keep = soroban_sdk::vec![&env, kept.clone()];
    client.clear_all_watchers(&admin, &keep);

    let events = env.events().all();
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

    assert_eq!(client.get_watcher_count(), 1);
    assert_eq!(client.get_watchers().len(), 1);
    assert!(client.is_watcher_authorized(&kept));
    assert!(!client.is_watcher_authorized(&w1));
    assert!(!client.is_watcher_authorized(&w2));
}

/// Regression test for #592: clearing all but an empty kept set is rejected
/// because it would leave the registry below `MIN_WATCHERS`.
#[test]
fn test_regression_clear_all_watchers_rejects_empty_kept_set() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    client.register_watcher(&admin, &w1);

    let empty: Vec<Address> = soroban_sdk::vec![&env];
    let res = client.try_clear_all_watchers(&admin, &empty);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::BelowMinWatchers);
    assert_eq!(client.get_watcher_count(), 1);
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

/// Regression test for #595: `replace_watcher` must carry the old watcher's
/// `WatcherInfo` and `WatcherScope` to the new address (and clear them from
/// the old one), so key rotation does not silently widen access.
#[test]
fn test_regression_replace_watcher_carries_info_and_scope() {
    let (env, admin, client) = setup();
    let old_watcher = Address::generate(&env);
    let new_watcher = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);

    client.register_watcher(&admin, &old_watcher);
    let info = WatcherInfo {
        label: String::from_str(&env, "keeper-east"),
        operator: String::from_str(&env, "Acme Monitoring"),
    };
    client.set_watcher_info(&admin, &old_watcher, &Some(info.clone()));
    let scope = soroban_sdk::vec![&env, target_a.clone()];
    client.set_watcher_scope(&admin, &old_watcher, &scope);

    client.replace_watcher(&admin, &old_watcher, &new_watcher);

    // Metadata and scope moved to the new address.
    assert_eq!(client.get_watcher_info(&new_watcher), Some(info));
    assert_eq!(client.get_watcher_scope(&new_watcher), scope);
    assert!(client.is_watcher_authorized_for(&new_watcher, &target_a));
    assert!(!client.is_watcher_authorized_for(&new_watcher, &target_b));

    // Old address no longer holds metadata or scope.
    assert!(client.get_watcher_info(&old_watcher).is_none());
    assert!(client.get_watcher_scope(&old_watcher).is_empty());

    // The `watcher.replace` event carries the scope.
    let events = env.events().all();
    let events = events.events();
    let mut found = false;
    for i in 0..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        if topics.len() == 2
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("watcher")
            && soroban_sdk::Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("replace")
        {
            let (old, new, emitted_scope): (Address, Address, Vec<Address>) =
                soroban_sdk::FromVal::from_val(&env, &data);
            assert_eq!(old, old_watcher);
            assert_eq!(new, new_watcher);
            assert_eq!(emitted_scope, scope);
            found = true;
        }
    }
    assert!(found, "watcher.replace event must be emitted");
}

/// Regression test for #596: clearing watcher metadata with `None` must emit
/// a `watcher.info` event (with an empty payload) so indexers drop stale data.
#[test]
fn test_regression_set_watcher_info_none_emits_event() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper-east"),
        operator: String::from_str(&env, "Acme Monitoring"),
    };
    client.set_watcher_info(&admin, &watcher, &Some(info));

    client.set_watcher_info(&admin, &watcher, &None);
    assert!(client.get_watcher_info(&watcher).is_none());

    let events = env.events().all();
    let events = events.events();
    let mut found = false;
    for i in 0..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        if topics.len() == 2
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("watcher")
            && soroban_sdk::Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("info")
        {
            let (emitted_watcher, emitted_info): (Address, Option<WatcherInfo>) =
                soroban_sdk::FromVal::from_val(&env, &data);
            assert_eq!(emitted_watcher, watcher);
            assert!(emitted_info.is_none());
            found = true;
        }
    }
    assert!(found, "watcher.info event must be emitted when clearing info");
}

/// Regression test for #598: `propose_admin_action` must emit an `admin.queue`
/// event carrying the queued `AdminAction` so co-admins can monitor the
/// timelock window without calling `get_pending_action`.
#[test]
fn test_regression_propose_admin_action_emits_queued_action() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    // Upgrade(hash)
    let hash = soroban_sdk::BytesN::from_array(&env, &[7u8; 32]);
    client.propose_admin_action(&admin, &AdminAction::Upgrade(hash.clone()));
    let events = env.events().all();
    let events = events.events();
    let mut found = false;
    for i in 0..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        if topics.len() == 2
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("admin")
            && soroban_sdk::Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("queue")
        {
            let (caller, _ready_at, action): (Address, u32, AdminAction) =
                soroban_sdk::FromVal::from_val(&env, &data);
            assert_eq!(caller, admin);
            assert_eq!(action, AdminAction::Upgrade(hash.clone()));
            found = true;
        }
    }
    assert!(found, "admin.queue event must be emitted for Upgrade");

    // TransferAdmin(addr)
    client.cancel_admin_action(&admin);
    client.propose_admin_action(&admin, &AdminAction::TransferAdmin(new_admin.clone()));
    let events = env.events().all();
    let events = events.events();
    let mut found = false;
    for i in 0..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        if topics.len() == 2
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("admin")
            && soroban_sdk::Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("queue")
        {
            let (caller, _ready_at, action): (Address, u32, AdminAction) =
                soroban_sdk::FromVal::from_val(&env, &data);
            assert_eq!(caller, admin);
            assert_eq!(action, AdminAction::TransferAdmin(new_admin.clone()));
            found = true;
        }
    }
    assert!(found, "admin.queue event must be emitted for TransferAdmin");
}

/// Regression test for #599: `are_watchers_authorized` accepts exactly
/// `MAX_PAGE_SIZE` entries and errors on larger batches instead of silently
/// truncating the result.
#[test]
fn test_regression_are_watchers_authorized_oversized_batch_errors() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    client.register_watcher(&admin, &w1);

    // Exactly MAX_PAGE_SIZE is accepted.
    let mut at_limit: Vec<Address> = soroban_sdk::vec![&env];
    for _ in 0..MAX_PAGE_SIZE {
        at_limit.push_back(w1.clone());
    }
    let result = client.are_watchers_authorized(&at_limit);
    assert_eq!(result.len() as u32, MAX_PAGE_SIZE);
    for i in 0..result.len() {
        assert!(result.get(i).unwrap());
    }

    // One past the limit is rejected.
    let mut over_limit: Vec<Address> = soroban_sdk::vec![&env];
    for _ in 0..(MAX_PAGE_SIZE + 1) {
        over_limit.push_back(w1.clone());
    }
    assert_eq!(
        client
            .try_are_watchers_authorized(&over_limit)
            .unwrap_err()
            .unwrap(),
        ContractError::BatchTooLarge
    );
/// Regression test for #594: removing a watcher must delete its metadata and
/// scope so a re-registered address starts unscoped and without metadata.
#[test]
fn test_regression_remove_watcher_clears_metadata_and_scope() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper"),
        operator: String::from_str(&env, "Acme"),
    };
    client.set_watcher_info(&admin, &watcher, &Some(info));
    client.set_watcher_scope(&admin, &watcher, &soroban_sdk::vec![&env, target.clone()]);
    assert!(!client.get_watcher_scope(&watcher).is_empty());

    client.remove_watcher(&admin, &watcher);
    assert!(client.get_watcher_info(&watcher).is_none());
    assert!(client.get_watcher_scope(&watcher).is_empty());

    // Re-registering the same address starts unscoped and without metadata.
    client.register_watcher(&admin, &watcher);
    assert!(client.get_watcher_info(&watcher).is_none());
    assert!(client.get_watcher_scope(&watcher).is_empty());
    assert!(client.is_watcher_authorized_for(&watcher, &target));
}

/// Regression test for #594: `clear_watchers_batch` must also delete the
/// metadata and scope of every removed watcher.
#[test]
fn test_regression_clear_watchers_batch_clears_metadata_and_scope() {
    let (env, admin, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_watcher(&admin, &w1);
    client.register_watcher(&admin, &w2);

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper"),
        operator: String::from_str(&env, "Acme"),
    };
    client.set_watcher_info(&admin, &w1, &Some(info));
    client.set_watcher_scope(&admin, &w1, &soroban_sdk::vec![&env, target.clone()]);

    client.clear_watchers_batch(&admin, &soroban_sdk::vec![&env, w1.clone(), w2.clone()]);

    assert!(client.get_watcher_info(&w1).is_none());
    assert!(client.get_watcher_scope(&w1).is_empty());
    assert!(client.get_watcher_info(&w2).is_none());
    assert!(client.get_watcher_scope(&w2).is_empty());
}

/// Regression test for #591: a lower timelock delay can be proposed through
/// `propose_admin_action` and applied once the timelock elapses.
#[test]
fn test_regression_propose_lower_timelock_delay_then_execute() {
    use soroban_sdk::testutils::Ledger as _;

    let (env, admin, client) = setup();
    client.set_timelock_delay(&admin, &100);
    assert_eq!(client.get_timelock_delay(), 100);

    client.propose_admin_action(&admin, &AdminAction::SetTimelockDelay(10));
    env.ledger().with_mut(|li| li.sequence_number += 101);
    client.execute_admin_action(&admin);
    assert_eq!(client.get_timelock_delay(), 10);
}

/// Regression test for #591: the timelock delay can be disabled (set to zero)
/// through `propose_admin_action` and executed after the timelock elapses.
#[test]
fn test_regression_propose_disable_timelock_delay() {
    use soroban_sdk::testutils::Ledger as _;

    let (env, admin, client) = setup();
    client.set_timelock_delay(&admin, &50);
    assert_eq!(client.get_timelock_delay(), 50);

    client.propose_admin_action(&admin, &AdminAction::SetTimelockDelay(0));
    env.ledger().with_mut(|li| li.sequence_number += 51);
    client.execute_admin_action(&admin);
    assert_eq!(client.get_timelock_delay(), 0);
}

/// Regression test for #591: the direct `set_timelock_delay` setter stays
/// raise-only and rejects lowering the delay.
#[test]
fn test_regression_set_timelock_delay_direct_is_raise_only() {
    let (_env, admin, client) = setup();
    client.set_timelock_delay(&admin, &100);

    let res = client.try_set_timelock_delay(&admin, &10);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::TimelockRequired);
    assert_eq!(client.get_timelock_delay(), 100);
}

/// Regression test for #601: the guardian can pause the registry but cannot
/// unpause it, and the admin can rotate the guardian.
#[test]
fn test_regression_guardian_can_pause_but_not_unpause() {
    let (env, admin, client) = setup();
    let guardian = Address::generate(&env);

    assert!(client.get_guardian().is_none());
    client.set_guardian(&admin, &Some(guardian.clone()));
    assert_eq!(client.get_guardian(), Some(guardian.clone()));

    client.pause(&guardian);
    assert!(client.is_paused());

    let res = client.try_unpause(&guardian);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
    assert!(client.is_paused());

    // Admin can still unpause and rotate the guardian.
    client.unpause(&admin);
    assert!(!client.is_paused());

    let new_guardian = Address::generate(&env);
    client.set_guardian(&admin, &Some(new_guardian.clone()));
    assert_eq!(client.get_guardian(), Some(new_guardian));

    client.set_guardian(&admin, &None);
    assert!(client.get_guardian().is_none());
}

/// Regression test for #601: only an admin may set the guardian.
#[test]
fn test_regression_set_guardian_requires_admin() {
    let (env, _admin, client) = setup();
    let not_admin = Address::generate(&env);
    let guardian = Address::generate(&env);

    let res = client.try_set_guardian(&not_admin, &Some(guardian));
    assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
    assert!(client.get_guardian().is_none());
/// Regression test for #593: `set_watcher_info` must reject unregistered
/// watchers, labels/operators exceeding the caps, and calls while paused.
#[test]
fn test_regression_set_watcher_info_validation() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let unregistered = Address::generate(&env);

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper-east"),
        operator: String::from_str(&env, "Acme Monitoring"),
    };

    // Unregistered watcher is rejected.
    assert_eq!(
        client
            .try_set_watcher_info(&admin, &unregistered, &Some(info.clone()))
            .unwrap_err()
            .unwrap(),
        ContractError::WatcherNotFound
    );

    client.register_watcher(&admin, &watcher);

    // Label at the cap is accepted; one past the cap is rejected.
    let max_label = String::from_str(&env, &"a".repeat(MAX_WATCHER_LABEL_LEN as usize));
    let too_long_label = String::from_str(&env, &"a".repeat(MAX_WATCHER_LABEL_LEN as usize + 1));
    assert_eq!(
        client
            .try_set_watcher_info(
                &admin,
                &watcher,
                &Some(WatcherInfo {
                    label: max_label,
                    operator: String::from_str(&env, "op"),
                }),
            )
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        client
            .try_set_watcher_info(
                &admin,
                &watcher,
                &Some(WatcherInfo {
                    label: too_long_label,
                    operator: String::from_str(&env, "op"),
                }),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::LabelTooLong
    );

    // Operator at the cap is accepted; one past the cap is rejected.
    let max_operator = String::from_str(&env, &"b".repeat(MAX_WATCHER_OPERATOR_LEN as usize));
    let too_long_operator =
        String::from_str(&env, &"b".repeat(MAX_WATCHER_OPERATOR_LEN as usize + 1));
    assert_eq!(
        client
            .try_set_watcher_info(
                &admin,
                &watcher,
                &Some(WatcherInfo {
                    label: String::from_str(&env, "lbl"),
                    operator: max_operator,
                }),
            )
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        client
            .try_set_watcher_info(
                &admin,
                &watcher,
                &Some(WatcherInfo {
                    label: String::from_str(&env, "lbl"),
                    operator: too_long_operator,
                }),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::OperatorTooLong
    );

    // Paused contract rejects the setter.
    client.pause(&admin);
    assert_eq!(
        client
            .try_set_watcher_info(&admin, &watcher, &Some(info))
            .unwrap_err()
            .unwrap(),
        ContractError::ContractPaused
    );
}

/// Regression test for #593: `set_watcher_scope` must reject unregistered
/// watchers, scope vectors exceeding the cap, and calls while paused.
#[test]
fn test_regression_set_watcher_scope_validation() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let unregistered = Address::generate(&env);

    let scope = soroban_sdk::vec![&env, Address::generate(&env)];

    // Unregistered watcher is rejected.
    assert_eq!(
        client
            .try_set_watcher_scope(&admin, &unregistered, &scope)
            .unwrap_err()
            .unwrap(),
        ContractError::WatcherNotFound
    );

    client.register_watcher(&admin, &watcher);

    // Scope at the cap is accepted; one past the cap is rejected.
    let mut max_scope: Vec<Address> = soroban_sdk::vec![&env];
    for _ in 0..MAX_WATCHER_SCOPE_LEN {
        max_scope.push_back(Address::generate(&env));
    }
    assert_eq!(
        client
            .try_set_watcher_scope(&admin, &watcher, &max_scope)
            .unwrap(),
        Ok(())
    );

    let mut too_long_scope = max_scope.clone();
    too_long_scope.push_back(Address::generate(&env));
    assert_eq!(
        client
            .try_set_watcher_scope(&admin, &watcher, &too_long_scope)
            .unwrap_err()
            .unwrap(),
        ContractError::ScopeTooLong
    );

    // Paused contract rejects the setter.
    client.pause(&admin);
    assert_eq!(
        client
            .try_set_watcher_scope(&admin, &watcher, &scope)
            .unwrap_err()
            .unwrap(),
        ContractError::ContractPaused
    );
}

/// Regression test for #593: watcher info and scope live in persistent
/// storage, survive across invocations, and have their TTL extended on write.
#[test]
fn test_regression_watcher_metadata_persistent_storage_and_ttl() {
    use soroban_sdk::testutils::storage::Persistent as _;
    use soroban_sdk::testutils::Ledger as _;

    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    let info = WatcherInfo {
        label: String::from_str(&env, "keeper-east"),
        operator: String::from_str(&env, "Acme Monitoring"),
    };
    client.set_watcher_info(&admin, &watcher, &Some(info.clone()));
    client.set_watcher_scope(&admin, &watcher, &soroban_sdk::vec![&env, target.clone()]);

    // Advance the ledger so the TTL is non-trivial, then re-write to extend it.
    env.ledger().with_mut(|li| li.sequence_number += 10);
    client.set_watcher_info(&admin, &watcher, &Some(info.clone()));

    let contract_id = client.address.clone();
    let info_key = DataKey::WatcherInfo(watcher.clone());
    let scope_key = DataKey::WatcherScope(watcher.clone());
    let info_ttl = env.as_contract(&contract_id, || env.storage().persistent().get_ttl(&info_key));
    let scope_ttl =
        env.as_contract(&contract_id, || env.storage().persistent().get_ttl(&scope_key));
    assert!(info_ttl > 0, "WatcherInfo TTL must be extended on write");
    assert!(scope_ttl > 0, "WatcherScope TTL must be extended on write");

    // Values survive across invocations.
    assert_eq!(client.get_watcher_info(&watcher), Some(info));
    assert_eq!(
        client.get_watcher_scope(&watcher),
        soroban_sdk::vec![&env, target.clone()]
    );
    assert!(client.is_watcher_authorized_for(&watcher, &target));
}

/// Test for #602: `get_configuration` returns a snapshot matching the
/// individual getters.
#[test]
fn test_get_configuration_matches_individual_getters() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    client.register_watcher(&admin, &watcher);
    client.set_timelock_delay(&admin, &42);

    let config = client.get_configuration();
    assert_eq!(config.admins, client.get_admins());
    assert_eq!(config.paused, client.is_paused());
    assert_eq!(config.timelock_delay, client.get_timelock_delay());
    assert_eq!(config.pending_action, client.get_pending_action());
    assert_eq!(
        config.pending_admin_transfer,
        client.get_pending_admin_transfer()
    );
    assert_eq!(config.watcher_count, client.get_watcher_count());

    // Reflects state changes.
    client.pause(&admin);
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);
    let config = client.get_configuration();
    assert!(config.paused);
    assert_eq!(
        config.pending_admin_transfer,
        client.get_pending_admin_transfer()
    );
    assert!(config.pending_admin_transfer.is_some());
}
/// Regression test for #403: `remove_admin` on an address that is not an
/// admin must return `ContractError::AdminNotFound` and must not rewrite the
/// admin set or emit an `admin.remove` event.
#[test]
fn test_regression_remove_admin_non_admin_target_errors() {
    let (env, admin, client) = setup();
    let second_admin = Address::generate(&env);
    client.add_admin(&admin, &second_admin);

    let not_an_admin = Address::generate(&env);
    let admins_before = client.get_admins();

    let res = client.try_remove_admin(&admin, &not_an_admin);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::AdminNotFound,
        "Removing a non-admin target must return AdminNotFound"
    );
    assert_eq!(
        client.get_admins(),
        admins_before,
        "Admin set must be unchanged when the target was never an admin"
    );
}

/// Regression test for #404: enabling the timelock (raising the delay above
/// `0`) must invalidate any admin transfer that was proposed while the
/// timelock was off, so it cannot later be accepted with no delay applied.
#[test]
fn test_regression_timelock_enable_clears_pending_admin_transfer() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin, &new_admin);
    client.set_timelock_delay(&admin, &10);

    let res = client.try_accept_admin_transfer(&new_admin);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::NoPendingTransfer,
        "Enabling the timelock must clear a transfer proposed before it was configured"
    );
    assert!(!client.is_admin(&new_admin));
}

/// Regression test for #405: a pending admin transfer proposed by an admin
/// who is subsequently removed must be invalidated, not left acceptable by
/// the proposed (potentially attacker-controlled) address.
#[test]
fn test_regression_pending_transfer_invalidated_on_proposer_removal() {
    let (env, admin, client) = setup();
    let second_admin = Address::generate(&env);
    client.add_admin(&admin, &second_admin);

    let attacker = Address::generate(&env);
    client.propose_admin_transfer(&second_admin, &attacker);

    client.remove_admin(&admin, &second_admin);

    let res = client.try_accept_admin_transfer(&attacker);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::NoPendingTransfer,
        "A transfer proposed by a removed admin must no longer be acceptable"
    );
    assert!(!client.is_admin(&attacker));
}

/// Regression test for #406: a queued `PendingAction` whose proposer is
/// subsequently removed from the admin set must be auto-cancelled, not left
/// executable by a remaining or colluding admin.
#[test]
fn test_regression_pending_action_cancelled_on_proposer_removal() {
    let (env, admin, client) = setup();
    let second_admin = Address::generate(&env);
    client.add_admin(&admin, &second_admin);

    let third_admin = Address::generate(&env);
    client.propose_admin_action(&second_admin, &AdminAction::AddAdmin(third_admin.clone()));
    assert!(client.get_pending_action().is_some());

    client.remove_admin(&admin, &second_admin);

    assert!(
        client.get_pending_action().is_none(),
        "A pending action proposed by a removed admin must be auto-cancelled"
    );

    let res = client.try_execute_admin_action(&admin);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::NoPendingAction);
    assert!(!client.is_admin(&third_admin));
}

/// Regression test for #590: `get_pending_admin_transfer` must decode the
/// stored `(proposer, new_admin, expires_at_ledger)` tuple and return the
/// proposed `new_admin`, rather than misreading it as `(Address, u32)`.
#[test]
fn test_regression_get_pending_admin_transfer_returns_new_admin() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    // No proposal yet.
    assert!(client.get_pending_admin_transfer().is_none());

    client.propose_admin_transfer(&admin, &new_admin);
    assert_eq!(
        client.get_pending_admin_transfer(),
        Some(new_admin.clone()),
        "Getter must return the proposed new_admin while a transfer is pending"
    );

    // After accepting, there is no pending transfer.
    client.accept_admin_transfer(&new_admin);
    assert!(client.is_admin(&new_admin));
    assert!(client.get_pending_admin_transfer().is_none());
}

/// Regression test for #590: an expired proposal must not be reported by
/// `get_pending_admin_transfer` (matching AlertRegistry), and accepting it
/// must still fail with `AdminTransferExpired`.
#[test]
fn test_regression_get_pending_admin_transfer_none_when_expired() {
    use soroban_sdk::testutils::Ledger as _;

    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);
    assert_eq!(client.get_pending_admin_transfer(), Some(new_admin.clone()));

    // One ledger past the expiry ledger.
    env.ledger()
        .with_mut(|li| li.sequence_number += ADMIN_TRANSFER_EXPIRY_LEDGERS + 1);

    assert!(
        client.get_pending_admin_transfer().is_none(),
        "Expired proposals must not be returned by the getter"
    );
    assert_eq!(
        client
            .try_accept_admin_transfer(&new_admin)
            .unwrap_err()
            .unwrap(),
        ContractError::AdminTransferExpired
    );
}

/// Regression test for #590: cancelling a pending transfer clears it, so the
/// getter returns `None` afterwards.
#[test]
fn test_regression_get_pending_admin_transfer_none_after_cancel() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin, &new_admin);
    assert_eq!(client.get_pending_admin_transfer(), Some(new_admin.clone()));

    client.cancel_admin_transfer(&admin);
    assert!(client.get_pending_admin_transfer().is_none());
    assert!(!client.is_admin(&new_admin));
/// Regression test for #597: while the registry is paused, a pending admin
/// transfer cannot be accepted, but it can still be cancelled.
#[test]
fn test_regression_accept_admin_transfer_blocked_while_paused() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin, &new_admin);
    client.pause(&admin);

    let res = client.try_accept_admin_transfer(&new_admin);
    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::Paused,
        "Accepting an admin transfer must be blocked while paused"
    );
    assert!(!client.is_admin(&new_admin));

    // Cancelling remains allowed while paused so a suspected takeover can be
    // aborted without unpausing.
    assert_eq!(
        client.try_cancel_admin_transfer(&admin).unwrap(),
        Ok(())
    );
    assert_eq!(
        client.try_accept_admin_transfer(&new_admin).unwrap_err().unwrap(),
        ContractError::NoPendingTransfer
    );
}

/// Test for #600: `set_watcher_scope` deduplicates entries and rejects the
/// registry's own address and the watcher's own address.
#[test]
fn test_set_watcher_scope_validates_targets() {
    let (env, admin, client) = setup();
    let watcher = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);
    client.register_watcher(&admin, &watcher);

    // Duplicates are collapsed while preserving order.
    let scope = soroban_sdk::vec![
        &env,
        target_a.clone(),
        target_b.clone(),
        target_a.clone(),
        target_b.clone(),
    ];
    client.set_watcher_scope(&admin, &watcher, &scope);
    let stored = client.get_watcher_scope(&watcher);
    assert_eq!(stored.len(), 2);
    assert_eq!(stored.get(0).unwrap(), target_a);
    assert_eq!(stored.get(1).unwrap(), target_b);

    // The registry's own address is not a valid target.
    let registry_id = client.address.clone();
    let res = client.try_set_watcher_scope(
        &admin,
        &watcher,
        &soroban_sdk::vec![&env, registry_id],
    );
    assert_eq!(res.unwrap_err().unwrap(), ContractError::InvalidScopeTarget);

    // The watcher itself is not a valid target.
    let res = client.try_set_watcher_scope(
        &admin,
        &watcher,
        &soroban_sdk::vec![&env, watcher.clone()],
    );
    assert_eq!(res.unwrap_err().unwrap(), ContractError::InvalidScopeTarget);

    // The previously stored scope is unchanged after rejected updates.
    assert_eq!(client.get_watcher_scope(&watcher), stored);
}

/// Test for #603: the constructor records the current schema version and
/// `migrate` validates its arguments.
#[test]
fn test_schema_version_and_migrate_validation() {
    let (_env, _admin, client) = setup();

    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION);

    // `from` must match the stored schema version.
    let res = client.try_migrate(&(CURRENT_SCHEMA_VERSION + 1), &CURRENT_SCHEMA_VERSION);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::SchemaMismatch);

    // `to` must be strictly greater than `from`.
    let res = client.try_migrate(&CURRENT_SCHEMA_VERSION, &CURRENT_SCHEMA_VERSION);
    assert_eq!(res.unwrap_err().unwrap(), ContractError::InvalidSchemaVersion);

    // A valid migration advances the stored version.
    assert_eq!(
        client
            .try_migrate(&CURRENT_SCHEMA_VERSION, &(CURRENT_SCHEMA_VERSION + 1))
            .unwrap(),
        Ok(())
    );
    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION + 1);
}
