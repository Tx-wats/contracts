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
