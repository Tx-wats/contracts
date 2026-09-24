use crate::AlertRegistry;
use crate::AlertRegistryClient;
use crate::ContractError;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    vec, Address, Env, FromVal, String, Symbol,
};

fn setup() -> (Env, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client)
}

fn hash64(env: &Env) -> String {
    hash64c(env, '0')
}

fn hash64c(env: &Env, c: char) -> String {
    let buf = [c as u8; 64];
    String::from_str(env, core::str::from_utf8(&buf).unwrap())
}

fn str(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

/// Regression test for historical bug:
/// `update_alert silently discarded rule validation.`
///
/// It called `validate_rules` and dropped the returned `Result`, so an alert could be
/// updated with rule descriptors that `register_alert` rejects. This test ensures
/// that invalid rule descriptors are rejected with `ContractError::InvalidRuleDescriptor`
/// and not silently ignored.
#[test]
fn test_regression_update_alert_discarding_rule_validation_errors() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Valid Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // Attempt to update with invalid rule descriptor
    let invalid_rules = vec![&env, str(&env, "invalid:rule:type")];
    let res = client.try_update_alert(&owner, &id, &invalid_rules, &true);

    assert_eq!(
        res.unwrap_err().unwrap(),
        ContractError::InvalidRuleDescriptor,
        "update_alert must propagate rule validation errors"
    );

    // Verify rules were not modified
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
}

/// Regression test for historical bug:
/// `AlertRegistry::remove_alert body was missing in lib.rs (structural corruption); restored with correct remove_alert_record call.`
///
/// Ensures that calling `remove_alert` fully purges the alert from primary storage,
/// the active status lookup, the owner index, and the target contract index, and emits the remove event.
#[test]
fn test_regression_missing_remove_alert_body() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "To Remove"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert!(client.get_alert(&owner, &id).is_some());
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 1);
    assert_eq!(client.get_alerts_for_contract(&owner, &target).len(), 1);

    let events_before = env.events().all().len();

    // Call remove_alert
    assert_eq!(client.try_remove_alert(&owner, &id).unwrap(), Ok(()));

    let all_events = env.events().all();
    assert!(
        all_events.len() > events_before,
        "Remove alert must emit an event"
    );

    // Must be completely cleaned up
    assert!(client.get_alert(&owner, &id).is_none());
    assert_eq!(client.get_alert_active(&owner, &id), None);
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 0);
    assert_eq!(client.get_alerts_for_contract(&owner, &target).len(), 0);
}

/// Regression test for historical bug:
/// `update_webhook accepted webhook hashes of any length, while register_alert required exactly 64 characters.`
///
/// Ensures that `update_webhook` enforces 64-character length validation identically to `register_alert`.
#[test]
fn test_regression_update_webhook_accepted_invalid_length_hashes() {
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

    // Too short (63 chars)
    let short_hash = str(
        &env,
        "123456789012345678901234567890123456789012345678901234567890123",
    );
    let res_short = client.try_update_webhook(&owner, &id, &short_hash);
    assert_eq!(
        res_short.unwrap_err().unwrap(),
        ContractError::InvalidWebhookHash
    );

    // Too long (65 chars)
    let long_hash = str(
        &env,
        "12345678901234567890123456789012345678901234567890123456789012345",
    );
    let res_long = client.try_update_webhook(&owner, &id, &long_hash);
    assert_eq!(
        res_long.unwrap_err().unwrap(),
        ContractError::InvalidWebhookHash
    );

    // Empty hash
    let empty_hash = str(&env, "");
    let res_empty = client.try_update_webhook(&owner, &id, &empty_hash);
    assert_eq!(
        res_empty.unwrap_err().unwrap(),
        ContractError::InvalidWebhookHash
    );

    // Valid 64-char hash succeeds
    let valid_hash = str(
        &env,
        "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd",
    );
    assert_eq!(
        client.try_update_webhook(&owner, &id, &valid_hash).unwrap(),
        Ok(())
    );
    assert_eq!(
        client.get_alert(&owner, &id).unwrap().webhook_hash,
        valid_hash
    );
}

/// Regression test for historical bug:
/// `configs_paginated could overflow on offset + limit; now saturating.`
///
/// Ensures large offset/limit combinations saturate rather than overflowing with arithmetic panic.
#[test]
fn test_regression_configs_paginated_overflow_on_offset_plus_limit() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 1"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // offset + limit would overflow u32::MAX
    let res = client.get_contract_alerts_paginated(&owner, &target, &u32::MAX, &10);
    assert_eq!(res.len(), 0);

    let res_owner = client.get_alerts_by_owner_paginated(&owner, &owner, &u32::MAX, &10);
    assert_eq!(res_owner.len(), 0);

    let res_large_limit = client.get_contract_alerts_paginated(&owner, &target, &0, &u32::MAX);
    assert_eq!(res_large_limit.len(), 1);
}

/// Regression test for historical bug:
/// `AlertRegistry::transfer_admin emitted no event, leaving a change of control invisible on-chain.`
///
/// Ensures `transfer_admin` emits `("admin", "transfer")` event with old and new admin addresses.
#[test]
fn test_regression_transfer_admin_emitted_no_event() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin);

    let events_before = env.events().all().len();
    assert_eq!(
        client.try_transfer_admin(&admin, &new_admin).unwrap(),
        Ok(())
    );

    let all_events = env.events().all();
    let new_events = all_events.slice(events_before..all_events.len());
    assert_eq!(new_events.len(), 1);

    let (_, topics, _) = new_events.get(0).unwrap();
    let first_symbol: Symbol = FromVal::from_val(&env, &topics.get(0).unwrap());
    let second_symbol: Symbol = FromVal::from_val(&env, &topics.get(1).unwrap());
    assert_eq!(first_symbol, symbol_short!("admin"));
    assert_eq!(second_symbol, symbol_short!("transfer"));
    assert_eq!(client.get_admin(), new_admin);
}

/// Regression test for historical bug:
/// `AlertRegistry::remove_alert_by_admin was missing from lib.rs; restored.`
///
/// Ensures admin can remove any alert and clean up all index entries.
#[test]
fn test_regression_remove_alert_by_admin_missing() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.initialize(&admin);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Admin Remove Target"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(
        client.try_remove_alert_by_admin(&admin, &id).unwrap(),
        Ok(())
    );
    assert!(client.get_alert(&owner, &id).is_none());
    assert_eq!(client.get_alert_active(&owner, &id), None);
    assert_eq!(client.get_alerts_by_owner(&admin, &owner).len(), 0);
    assert_eq!(client.get_alerts_for_contract(&admin, &target).len(), 0);
}

/// Regression test for historical bug:
/// `AlertRegistry::update_alert now keeps DataKey::AlertActive in sync when active changes.`
///
/// Ensures toggling `active` bool via `update_alert` correctly synchronizes `DataKey::AlertActive`.
#[test]
fn test_regression_update_alert_keeps_alert_active_in_sync() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Sync Test"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // Initial state: active = true
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));

    // Deactivate
    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &false);
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));
    assert!(!client.get_alert(&owner, &id).unwrap().active);

    // Reactivate
    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &true);
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
    assert!(client.get_alert(&owner, &id).unwrap().active);
}

/// Regression test for historical bug:
/// `renew_alert_ttl — owner-authenticated TTL extension that leaves updated_at untouched.`
///
/// Ensures `renew_alert_ttl` does NOT advance `updated_at`, preserving sync integrity for incremental syncs.
#[test]
fn test_regression_renew_alert_ttl_preserves_updated_at() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "TTL Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let cfg_initial = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg_initial.created_at, 1000);
    assert_eq!(cfg_initial.updated_at, 1000);

    // Advance ledger time significantly
    env.ledger().set_timestamp(5000);

    assert_eq!(client.try_renew_alert_ttl(&owner, &id).unwrap(), Ok(()));

    let cfg_after_renew = client.get_alert(&owner, &id).unwrap();
    assert_eq!(
        cfg_after_renew.updated_at, 1000,
        "renew_alert_ttl must NOT modify updated_at"
    );
}

/// Regression test for #216:
/// `update_webhook left a stale pending hash that later overwrote it.`
///
/// `propose_webhook(B)`, then `update_webhook(C)`, then `confirm_webhook()` used
/// to promote the stale `B` over the direct update to `C`. A direct update now
/// discards the staged rotation, so the confirm has nothing to promote.
#[test]
fn test_regression_update_webhook_clears_stale_pending_hash() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Rotating Alert"),
        &hash64c(&env, 'a'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));
    client.update_webhook(&owner, &id, &hash64c(&env, 'c'));

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.webhook_hash, hash64c(&env, 'c'));
    assert!(
        cfg.pending_webhook_hash.is_none(),
        "update_webhook must discard the staged rotation"
    );

    assert_eq!(
        client.try_confirm_webhook(&owner, &id).unwrap_err().unwrap(),
        ContractError::NoPendingWebhook
    );
    assert_eq!(
        client.get_alert(&owner, &id).unwrap().webhook_hash,
        hash64c(&env, 'c'),
        "the direct update must survive a later confirm attempt"
    );
}

/// Mirror of the pre-#212 key layout, used to seed storage exactly as a
/// contract deployed before the `OwnerActiveCount` → `OwnerLiveCount` rename
/// would have left it.
#[soroban_sdk::contracttype]
enum LegacyDataKey {
    OwnerActiveCount(Address),
}

/// Regression test for #212:
/// renaming `DataKey::OwnerActiveCount` to `OwnerLiveCount` changes the
/// on-chain key encoding, so counters written by the old build must be
/// migrated rather than silently reset to zero (which would let an owner
/// exceed the per-owner alert limit).
#[test]
fn test_regression_owner_live_count_migrates_legacy_key() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "Pre-upgrade Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // Rewind the counter to how the old build stored it: under the legacy key only.
    env.as_contract(&client.address, || {
        let storage = env.storage().persistent();
        storage.remove(&crate::DataKey::OwnerLiveCount(owner.clone()));
        storage.set(&LegacyDataKey::OwnerActiveCount(owner.clone()), &5u32);
    });

    assert_eq!(client.get_non_removed_alert_count(&owner), 5);

    env.as_contract(&client.address, || {
        let storage = env.storage().persistent();
        assert!(
            !storage.has(&LegacyDataKey::OwnerActiveCount(owner.clone())),
            "the legacy entry must be removed once migrated"
        );
        assert_eq!(
            storage.get::<_, u32>(&crate::DataKey::OwnerLiveCount(owner.clone())),
            Some(5)
        );
    });

    // Later writes build on the migrated value rather than restarting at zero.
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Post-upgrade Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert_eq!(client.get_non_removed_alert_count(&owner), 6);
}
