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

fn hash64(env: &Env) -> soroban_sdk::BytesN<32> {
    hash64c(env, '0')
}

/// A webhook hash (32-byte SHA-256 digest) with every byte set to `c`.
fn hash64c(env: &Env, c: char) -> soroban_sdk::BytesN<32> {
    soroban_sdk::BytesN::from_array(env, &[c as u8; 32])
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
/// Since #214 every entry point takes the hash as `BytesN<32>`, so a
/// wrong-length hash can no longer be constructed at all. What remains to
/// check is that the 32 digest bytes are stored and returned unchanged.
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

    let digest: [u8; 32] = core::array::from_fn(|i| i as u8);
    let new_hash = soroban_sdk::BytesN::from_array(&env, &digest);
    assert_eq!(
        client.try_update_webhook(&owner, &id, &new_hash).unwrap(),
        Ok(())
    );
    assert_eq!(
        client
            .get_alert(&owner, &id)
            .unwrap()
            .webhook_hash
            .to_array(),
        digest
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
        client
            .try_confirm_webhook(&owner, &id)
            .unwrap_err()
            .unwrap(),
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

/// Remaining TTL of each entry an alert depends on, in the order
/// `Alert`, `AlertActive`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex`.
fn alert_entry_ttls(
    env: &Env,
    client: &AlertRegistryClient,
    id: u64,
    owner: &Address,
    target: &Address,
) -> [u32; 5] {
    use soroban_sdk::testutils::storage::Persistent as _;

    env.as_contract(&client.address, || {
        let storage = env.storage().persistent();
        [
            storage.get_ttl(&crate::DataKey::Alert(id)),
            storage.get_ttl(&crate::DataKey::AlertActive(id)),
            storage.get_ttl(&crate::DataKey::OwnerIndex(owner.clone())),
            storage.get_ttl(&crate::DataKey::OwnerLiveCount(owner.clone())),
            storage.get_ttl(&crate::DataKey::ContractIndex(target.clone())),
        ]
    })
}

/// Regression test for #213:
/// every mutator used to hand-copy its own `extend_ttl` calls, and each TTL
/// bug so far was one copy drifting from the rest. All mutators now go
/// through `persist_alert`/`touch_alert`, so after any of them every entry
/// the alert depends on must be back at the full `DEFAULT_TTL`.
#[test]
fn test_regression_every_mutator_refreshes_all_alert_ttls() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "TTL Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let full = [crate::DEFAULT_TTL; 5];
    assert_eq!(alert_entry_ttls(&env, &client, id, &owner, &target), full);

    // Each step ages every entry, runs one mutator, and expects a full refresh.
    let age = |env: &Env| env.ledger().with_mut(|li| li.sequence_number += 1_000);

    age(&env);
    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "update_alert"
    );

    age(&env);
    client.update_label(&owner, &id, &str(&env, "Renamed"));
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "update_label"
    );

    age(&env);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "propose_webhook"
    );

    age(&env);
    client.confirm_webhook(&owner, &id);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "confirm_webhook"
    );

    age(&env);
    client.update_webhook(&owner, &id, &hash64c(&env, 'c'));
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "update_webhook"
    );

    age(&env);
    client.renew_alert_ttl(&owner, &id);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "renew_alert_ttl"
    );

    age(&env);
    client.deactivate_alert_by_admin(&admin, &id);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &target),
        full,
        "deactivate_alert_by_admin"
    );

    age(&env);
    client.update_target_contract(&owner, &id, &new_target);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &owner, &new_target),
        full,
        "update_target_contract"
    );

    age(&env);
    client.transfer_alert_ownership(&owner, &id, &new_owner);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &new_owner, &new_target),
        full,
        "transfer_alert_ownership"
    );
}

/// Regression test for #204:
/// `deactivate_all_alerts` returned a plain `0` while paused, which callers
/// could not tell apart from "owner had no active alerts". It now returns
/// `Err(Paused)` like every other mutator and leaves the alerts untouched.
#[test]
fn test_regression_deactivate_all_alerts_rejects_while_paused() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Paused Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    client.pause(&admin);
    assert_eq!(
        client
            .try_deactivate_all_alerts(&owner)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );
    assert!(
        client.get_alert(&owner, &id).unwrap().active,
        "a rejected call must not deactivate anything"
    );

    client.unpause(&admin);
    assert_eq!(client.deactivate_all_alerts(&owner), 1);
    assert!(!client.get_alert(&owner, &id).unwrap().active);
}

/// Remaining TTL of the contract's instance entry.
fn instance_ttl(env: &Env, client: &AlertRegistryClient) -> u32 {
    use soroban_sdk::testutils::storage::Instance as _;
    env.as_contract(&client.address, || env.storage().instance().get_ttl())
}

fn advance_ledgers(env: &Env, ledgers: u32) {
    env.ledger().with_mut(|li| li.sequence_number += ledgers);
}

/// Regression test for #206 (ledger advancement, see #266):
/// AlertRegistry never extended its instance entry, so a deployment that
/// went quiet would archive it and take the admin, the ID counter and the
/// limits with it. `bump_instance_ttl` lets a keeper extend it to the full
/// `INSTANCE_BUMP_AMOUNT` before it expires.
#[test]
fn test_regression_bump_instance_ttl_keeps_instance_alive() {
    let (env, client) = setup();
    let initial_ttl = instance_ttl(&env, &client);
    assert!(initial_ttl < crate::INSTANCE_BUMP_THRESHOLD);

    // Just before expiry, a permissionless bump restores the full TTL.
    advance_ledgers(&env, initial_ttl - 1);
    client.bump_instance_ttl();
    assert_eq!(instance_ttl(&env, &client), crate::INSTANCE_BUMP_AMOUNT);

    // Well past the point where the un-bumped entry would have expired, the
    // instance is still live with the expected remaining lifetime. (The test
    // host does not archive entries, so the TTL itself is the assertion.)
    let past_original_expiry = initial_ttl + 1_000;
    advance_ledgers(&env, past_original_expiry);
    assert_eq!(
        instance_ttl(&env, &client),
        crate::INSTANCE_BUMP_AMOUNT - past_original_expiry
    );
}

/// Regression test for #206: every write to instance storage (the ID counter
/// in `next_id` and the admin setters) extends the instance entry.
#[test]
fn test_regression_instance_writes_extend_instance_ttl() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.initialize(&admin);
    assert_eq!(instance_ttl(&env, &client), crate::INSTANCE_BUMP_AMOUNT);

    // Age the entry below the bump threshold, then write through next_id.
    let age_below_threshold = crate::INSTANCE_BUMP_AMOUNT - crate::INSTANCE_BUMP_THRESHOLD + 1;
    advance_ledgers(&env, age_below_threshold);
    assert!(instance_ttl(&env, &client) < crate::INSTANCE_BUMP_THRESHOLD);
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Counter Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert_eq!(
        instance_ttl(&env, &client),
        crate::INSTANCE_BUMP_AMOUNT,
        "register_alert"
    );

    // Same for an admin setter.
    advance_ledgers(&env, age_below_threshold);
    client.set_global_alert_limit(&admin, &100);
    assert_eq!(
        instance_ttl(&env, &client),
        crate::INSTANCE_BUMP_AMOUNT,
        "set_global_alert_limit"
    );
}

/// Remaining TTL of a persistent entry in the registry's storage.
fn persistent_ttl(env: &Env, client: &AlertRegistryClient, key: &crate::DataKey) -> u32 {
    use soroban_sdk::testutils::storage::Persistent as _;
    env.as_contract(&client.address, || env.storage().persistent().get_ttl(key))
}

/// Regression test for #210:
/// `deactivate_alert_by_admin`, `update_target_contract` and
/// `transfer_alert_ownership` must refresh every index the alert belongs to,
/// including the index it is moved **out of**. The owner/target it is moved
/// into is covered by `test_regression_every_mutator_refreshes_all_alert_ttls`;
/// this covers the index left behind, which still holds the owner's or
/// target's other alerts.
#[test]
fn test_regression_mutators_refresh_indexes_they_leave() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);
    let register = |label: &str| {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        )
    };
    let moved = register("Moved");
    let stays = register("Stays");
    let age = |env: &Env| env.ledger().with_mut(|li| li.sequence_number += 1_000);
    let full = crate::DEFAULT_TTL;

    // Admin deactivation refreshes both indexes the alert is in.
    age(&env);
    client.deactivate_alert_by_admin(&admin, &stays);
    assert_eq!(
        persistent_ttl(&env, &client, &crate::DataKey::OwnerIndex(owner.clone())),
        full,
        "deactivate_alert_by_admin: owner index"
    );
    assert_eq!(
        persistent_ttl(
            &env,
            &client,
            &crate::DataKey::ContractIndex(target.clone())
        ),
        full,
        "deactivate_alert_by_admin: contract index"
    );

    // Retargeting refreshes the old target's index, which still holds `stays`.
    age(&env);
    client.update_target_contract(&owner, &moved, &new_target);
    assert_eq!(
        persistent_ttl(
            &env,
            &client,
            &crate::DataKey::ContractIndex(target.clone())
        ),
        full,
        "update_target_contract: old contract index"
    );
    assert_eq!(
        persistent_ttl(&env, &client, &crate::DataKey::OwnerIndex(owner.clone())),
        full,
        "update_target_contract: owner index"
    );

    // Transferring refreshes the old owner's index and live counter.
    age(&env);
    client.transfer_alert_ownership(&owner, &moved, &new_owner);
    assert_eq!(
        persistent_ttl(&env, &client, &crate::DataKey::OwnerIndex(owner.clone())),
        full,
        "transfer_alert_ownership: old owner index"
    );
    assert_eq!(
        persistent_ttl(
            &env,
            &client,
            &crate::DataKey::OwnerLiveCount(owner.clone())
        ),
        full,
        "transfer_alert_ownership: old owner live count"
    );
    assert_eq!(
        persistent_ttl(
            &env,
            &client,
            &crate::DataKey::ContractIndex(new_target.clone())
        ),
        full,
        "transfer_alert_ownership: contract index"
    );
}

/// Regression test for #211:
/// instance keys are now read and written through the `instance_key`
/// constants instead of ad-hoc `symbol_short!` literals. The constants must
/// stay byte-identical to the literals, or an upgraded contract would lose the
/// admin, counter and limits already stored by a deployed one.
#[test]
fn test_regression_instance_keys_match_legacy_symbols() {
    use crate::instance_key;
    use soroban_sdk::symbol_short;

    assert_eq!(instance_key::ADMIN, symbol_short!("ADMIN"));
    assert_eq!(instance_key::NEXT_ID, symbol_short!("NEXT_ID"));
    assert_eq!(instance_key::PAUSED, symbol_short!("PAUSED"));
    assert_eq!(instance_key::LIMIT, symbol_short!("LIMIT"));
    assert_eq!(instance_key::CLIMIT, symbol_short!("CLIMIT"));
    assert_eq!(instance_key::GLIMIT, symbol_short!("GLIMIT"));
    assert_eq!(instance_key::WATCHREG, symbol_short!("WATCHREG"));

    // State written under the literal keys (as a deployed pre-#211 build
    // would have left it) is read back through the new constants.
    let (env, client) = setup();
    let admin = Address::generate(&env);
    env.as_contract(&client.address, || {
        let storage = env.storage().instance();
        storage.set(&symbol_short!("ADMIN"), &admin);
        storage.set(&symbol_short!("NEXT_ID"), &7u64);
        storage.set(&symbol_short!("LIMIT"), &3u32);
        storage.set(&symbol_short!("PAUSED"), &true);
    });

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_alert_count(), 7);
    assert_eq!(client.get_per_owner_alert_limit(), 3);
    assert!(client.is_paused());
}
