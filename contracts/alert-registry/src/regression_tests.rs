// The unpaginated getters are deprecated (#384) but still part of the
// contract ABI; these tests pin their behaviour until they are removed.
#![allow(deprecated)]

use crate::AlertRegistry;
use crate::AlertRegistryClient;
use crate::ContractError;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    vec, Address, Env, FromVal, String, Symbol,
};

fn setup() -> (Env, Address, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(AlertRegistry, (&admin,));
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, admin, client)
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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);


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
    let (env, admin, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);


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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, _admin, client) = setup();
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
    let (env, admin, client) = setup();
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
    client.propose_alert_transfer(&owner, &id, &new_owner);
    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(
        alert_entry_ttls(&env, &client, id, &new_owner, &new_target),
        full,
        "accept_alert_transfer"
    );
}

// ── #201: recipient must accept alert ownership transfers ────────────────────

/// Register one alert for a fresh owner; returns `(env, client, owner, target, id)`.
fn transfer_fixture() -> (Env, AlertRegistryClient<'static>, Address, Address, u64) {
    let (env, _admin, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Transferable"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    (env, client, owner, target, id)
}

/// Regression test for #201: proposing a transfer changes nothing until the
/// recipient accepts, so alerts can no longer be pushed onto a victim's
/// address (filling their quota and polluting their alert list).
#[test]
fn test_regression_alert_transfer_requires_recipient_acceptance() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let victim = Address::generate(&env);

    client.propose_alert_transfer(&owner, &id, &victim);
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, owner);
    assert_eq!(client.get_alert_ids_by_owner(&victim).len(), 0);
    assert_eq!(client.get_non_removed_alert_count(&victim), 0);

    // Only the named recipient can accept.
    let stranger = Address::generate(&env);
    assert_eq!(
        client
            .try_accept_alert_transfer(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    client.accept_alert_transfer(&victim, &id);
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, victim);
    assert_eq!(client.get_alert_ids_by_owner(&victim), vec![&env, id]);
    assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 0);
    assert!(client.get_pending_alert_transfer(&id).is_none());
}

#[test]
fn test_regression_alert_transfer_accept_requires_recipient_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, (&Address::generate(&env),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    env.mock_all_auths();
    let id = client.register_alert(
        &owner,
        &Address::generate(&env),
        &str(&env, "Auth"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.propose_alert_transfer(&owner, &id, &new_owner);

    client.accept_alert_transfer(&new_owner, &id);
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(
        auths[0].0, new_owner,
        "accept must be authorised by the recipient"
    );
}

#[test]
fn test_regression_alert_transfer_reject_and_cancel() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let recipient = Address::generate(&env);

    // Recipient declines.
    client.propose_alert_transfer(&owner, &id, &recipient);
    assert_eq!(
        client
            .try_reject_alert_transfer(&owner, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized,
        "only the recipient can reject"
    );
    client.reject_alert_transfer(&recipient, &id);
    assert!(client.get_pending_alert_transfer(&id).is_none());
    assert_eq!(
        client
            .try_accept_alert_transfer(&recipient, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NoPendingTransfer
    );

    // Owner withdraws.
    client.propose_alert_transfer(&owner, &id, &recipient);
    client.cancel_alert_transfer(&owner, &id);
    assert!(client.get_pending_alert_transfer(&id).is_none());
    assert_eq!(
        client
            .try_cancel_alert_transfer(&owner, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NoPendingTransfer
    );
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, owner);
}

#[test]
fn test_regression_alert_transfer_expires() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let recipient = Address::generate(&env);

    client.propose_alert_transfer(&owner, &id, &recipient);
    let pending = client.get_pending_alert_transfer(&id).unwrap();
    assert_eq!(
        pending.expires_at_ledger,
        env.ledger().sequence() + crate::ALERT_TRANSFER_EXPIRY_LEDGERS
    );

    // One ledger past the window it can no longer be accepted...
    env.ledger()
        .with_mut(|li| li.sequence_number = pending.expires_at_ledger + 1);
    assert_eq!(
        client
            .try_accept_alert_transfer(&recipient, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::TransferExpired
    );
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, owner);

    // ...and a fresh proposal restarts the window.
    client.propose_alert_transfer(&owner, &id, &recipient);
    client.accept_alert_transfer(&recipient, &id);
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, recipient);
}

#[test]
fn test_regression_alert_transfer_accepted_on_last_ledger() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let recipient = Address::generate(&env);

    client.propose_alert_transfer(&owner, &id, &recipient);
    let expires = client
        .get_pending_alert_transfer(&id)
        .unwrap()
        .expires_at_ledger;
    env.ledger().with_mut(|li| li.sequence_number = expires);
    client.accept_alert_transfer(&recipient, &id);
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, recipient);
}

#[test]
fn test_regression_pending_transfer_cleared_on_remove_and_retarget() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let recipient = Address::generate(&env);

    client.propose_alert_transfer(&owner, &id, &recipient);
    client.update_target_contract(&owner, &id, &Address::generate(&env));
    assert!(
        client.get_pending_alert_transfer(&id).is_none(),
        "retargeting must clear the pending transfer"
    );

    client.propose_alert_transfer(&owner, &id, &recipient);
    client.remove_alert(&owner, &id);
    assert!(
        client.get_pending_alert_transfer(&id).is_none(),
        "removing the alert must clear the pending transfer"
    );
}

#[test]
fn test_regression_alert_transfer_rejects_self_and_paused() {
    let (env, client, owner, _target, id) = transfer_fixture();
    assert_eq!(
        client
            .try_propose_alert_transfer(&owner, &id, &owner)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransferRecipient
    );

    let admin = client.get_admin();
    let recipient = Address::generate(&env);
    client.propose_alert_transfer(&owner, &id, &recipient);
    client.pause(&admin);
    assert_eq!(
        client
            .try_accept_alert_transfer(&recipient, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );
}

/// Advance the ledger to one ledger short of `DEFAULT_TTL`, i.e. the last
/// ledger at which an entry written or extended `DEFAULT_TTL` ago is still live.
fn advance_almost_default_ttl(env: &Env) {
    env.ledger()
        .with_mut(|li| li.sequence_number += crate::DEFAULT_TTL - 1);
}

/// Regression test for #207 (ledger advancement):
/// the per-owner live counter used to be extended only when an alert was
/// registered or removed, so an owner who only bumped or renewed their alert
/// saw the counter expire after ~24 hours, read `0`, and could register a
/// whole new quota. Keep an alert alive across several TTL periods using only
/// `bump_alert` and `renew_alert_ttl`: the counter must stay live and the
/// per-owner limit must still hold.
#[test]
fn test_regression_owner_live_count_survives_keepalive_only_activity() {
    let (env, admin, client) = setup();
    client.set_per_owner_alert_limit(&admin, &1);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Kept Alive"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    for period in 0..4 {
        advance_almost_default_ttl(&env);
        if period % 2 == 0 {
            client.bump_alert(&id, &crate::DEFAULT_TTL);
        } else {
            client.renew_alert_ttl(&owner, &id);
        }
        let [_, _, _, live_count_ttl, _] = alert_entry_ttls(&env, &client, id, &owner, &target);
        assert_eq!(
            live_count_ttl,
            crate::DEFAULT_TTL,
            "OwnerLiveCount TTL must be refreshed in period {period}"
        );
    }

    assert_eq!(client.get_non_removed_alert_count(&owner), 1);
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Second"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:transfer")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::OwnerAlertLimitExceeded,
        "the limit must still apply after the original counter write would have expired"
    );
}

/// Regression test for #208 (ledger advancement):
/// most mutators used to extend `Alert(id)` but not the cheap `AlertActive(id)`
/// flag, so the flag could expire while the alert lived on, making
/// `get_alert_active` return `None` and `get_active_alert_count` undercount.
/// Edit the alert repeatedly while advancing the ledger past `DEFAULT_TTL` in
/// total: the flag must stay live and correct throughout.
#[test]
fn test_regression_alert_active_survives_repeated_edits() {
    let (env, _admin, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Edited"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let check = |step: &str| {
        let [_, active_ttl, _, _, _] = alert_entry_ttls(&env, &client, id, &owner, &target);
        assert_eq!(
            active_ttl,
            crate::DEFAULT_TTL,
            "AlertActive TTL after {step}"
        );
        assert_eq!(client.get_alert_active(&owner, &id), Some(true), "{step}");
        assert_eq!(client.get_active_alert_count(&owner), 1, "{step}");
    };

    advance_almost_default_ttl(&env);
    client.update_label(&owner, &id, &str(&env, "Renamed"));
    check("update_label");

    advance_almost_default_ttl(&env);
    client.update_webhook(&owner, &id, &hash64c(&env, 'b'));
    check("update_webhook");

    advance_almost_default_ttl(&env);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'c'));
    check("propose_webhook");

    advance_almost_default_ttl(&env);
    client.confirm_webhook(&owner, &id);
    check("confirm_webhook");

    advance_almost_default_ttl(&env);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'd'));
    client.cancel_webhook_proposal(&owner, &id);
    check("cancel_webhook_proposal");
}

// ── #209: expired alerts must not consume the owner's quota forever ──────────

/// Simulate an alert record expiring (as opposed to being removed): the
/// `Alert(id)` entry disappears while its index entries, live counter and
/// `AlertActive` flag are left behind. The test host never archives entries on
/// its own, so the expiry is applied directly.
fn expire_alert_record(env: &Env, client: &AlertRegistryClient, id: u64) {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .remove(&crate::DataKey::Alert(id));
    });
}

fn register(env: &Env, client: &AlertRegistryClient, owner: &Address, label: &str) -> u64 {
    client.register_alert(
        owner,
        &Address::generate(env),
        &str(env, label),
        &hash64(env),
        &vec![env, str(env, "rule:transfer")],
    )
}

/// Regression test for #209 (expiry, then prune, then register): expired
/// alerts used to keep counting against the per-owner limit, locking the owner
/// out of `register_alert` for good. Hitting the limit now prunes expired IDs
/// first, so the owner can register again.
#[test]
fn test_regression_expired_alerts_release_quota_on_register() {
    let (env, admin, client) = setup();
    client.set_per_owner_alert_limit(&admin, &2);
    let owner = Address::generate(&env);

    let expired = register(&env, &client, &owner, "Expires");
    let kept = register(&env, &client, &owner, "Kept");
    expire_alert_record(&env, &client, expired);
    assert_eq!(client.get_non_removed_alert_count(&owner), 2);

    let replacement = register(&env, &client, &owner, "Replacement");

    assert_eq!(
        client.get_alert_ids_by_owner(&owner),
        vec![&env, kept, replacement]
    );
    assert_eq!(client.get_non_removed_alert_count(&owner), 2);
    assert_eq!(client.get_alert_active(&owner, &expired), None);

    // At the limit with nothing expired, registration is still refused.
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &Address::generate(&env),
                &str(&env, "Over"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:transfer")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::OwnerAlertLimitExceeded
    );
}

/// Regression test for #209: `remove_alert` used to return `AlertNotFound`
/// for an expired alert, so the owner could not clean it up. It now drops the
/// dangling entry from the caller's own index.
#[test]
fn test_regression_remove_alert_cleans_up_expired_record() {
    let (env, _admin, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let id = register(&env, &client, &owner, "Expires");
    expire_alert_record(&env, &client, id);

    // Only the owner whose index holds the ID can clean it up.
    assert_eq!(
        client
            .try_remove_alert(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
    assert_eq!(client.get_non_removed_alert_count(&owner), 1);

    client.remove_alert(&owner, &id);
    assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 0);
    assert_eq!(client.get_non_removed_alert_count(&owner), 0);
    assert_eq!(client.get_alert_active(&owner, &id), None);

    // Once cleaned up, the ID is gone for good.
    assert_eq!(
        client.try_remove_alert(&owner, &id).unwrap_err().unwrap(),
        ContractError::AlertNotFound
    );
}

/// Regression test for #209: `prune_expired_alerts` is a permissionless
/// clean-up that only touches IDs whose record is gone.
#[test]
fn test_regression_prune_expired_alerts() {
    let (env, _admin, client) = setup();
    let owner = Address::generate(&env);
    let a = register(&env, &client, &owner, "A");
    let b = register(&env, &client, &owner, "B");
    let c = register(&env, &client, &owner, "C");
    expire_alert_record(&env, &client, a);
    expire_alert_record(&env, &client, c);

    assert_eq!(client.prune_expired_alerts(&owner), 2);
    assert_eq!(client.get_alert_ids_by_owner(&owner), vec![&env, b]);
    assert_eq!(client.get_non_removed_alert_count(&owner), 1);
    assert!(
        client.get_alert(&owner, &b).is_some(),
        "live alerts are untouched"
    );

    assert_eq!(
        client.prune_expired_alerts(&owner),
        0,
        "nothing left to prune"
    );
}

// ── #199: retargeting respects the per-contract limit ────────────────────────

/// Regression test for #385: `get_active_alert_count` used to scan the owner's
/// whole index on every call, which is unbounded work. It now reads a counter
/// that every mutator keeps in step, so the value must track registration,
/// activation toggles, admin suspension, removal, and bulk deactivation exactly.
#[test]
fn test_regression_get_active_alert_count_tracks_mutations() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);

    let a = register(&env, &client, &owner, "A");
    let b = register(&env, &client, &owner, "B");
    assert_eq!(client.get_active_alert_count(&owner), 2);
    assert_eq!(client.get_non_removed_alert_count(&owner), 2);

    // A toggle is reflected without a rescan.
    client.set_alert_active(&owner, &a, &false);
    assert_eq!(client.get_active_alert_count(&owner), 1);
    // A redundant toggle must not skew the counter.
    client.set_alert_active(&owner, &a, &false);
    assert_eq!(client.get_active_alert_count(&owner), 1);
    client.set_alert_active(&owner, &a, &true);
    assert_eq!(client.get_active_alert_count(&owner), 2);

    // A rules rewrite that flips `active` counts too.
    client.update_alert(&owner, &b, &vec![&env, str(&env, "rule:transfer")], &false);
    assert_eq!(client.get_active_alert_count(&owner), 1);

    // Admin suspension drops the alert from the active count.
    client.deactivate_alert_by_admin(&admin, &a);
    assert_eq!(client.get_active_alert_count(&owner), 0);

    // Removal keeps it at zero.
    client.remove_alert(&owner, &b);
    assert_eq!(client.get_active_alert_count(&owner), 0);

    // Bulk deactivation adjusts once per pass.
    register(&env, &client, &owner, "C");
    register(&env, &client, &owner, "D");
    assert_eq!(client.get_active_alert_count(&owner), 2);
    assert_eq!(client.deactivate_all_alerts(&owner), 2);
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

/// Regression test for #385 (transfer path): moving an active alert between
/// owners must migrate the active counter so neither owner's count drifts.
#[test]
fn test_regression_active_count_follows_transfer() {
    let (env, client, owner, _target, id) = transfer_fixture();
    let recipient = Address::generate(&env);

    assert_eq!(client.get_active_alert_count(&owner), 1);

    client.propose_alert_transfer(&owner, &id, &recipient);
    client.accept_alert_transfer(&recipient, &id);

    assert_eq!(client.get_active_alert_count(&owner), 0);
    assert_eq!(client.get_active_alert_count(&recipient), 1);
/// Regression test for #383: `get_alert_ids_by_owner` returned the raw owner
/// index, so an ID whose record had already been removed was still reported as
/// if the alert existed. It now filters out those dangling IDs.
#[test]
fn test_regression_get_alert_ids_by_owner_filters_dangling() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let keep = client.register_alert(
        &owner,
        &target,
        &str(&env, "Keep"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let remove = client.register_alert(
        &owner,
        &Address::generate(&env),
        &str(&env, "Remove"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:mint")],
    );

    assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 2);
    client.remove_alert(&owner, &remove);

    let ids = client.get_alert_ids_by_owner(&owner);
    assert_eq!(ids.len(), 1, "removed alert must not be reported as live");
    assert_eq!(ids.get(0).unwrap(), keep);
    // The live-counter path agrees with the filtered list.
    assert_eq!(client.get_non_removed_alert_count(&owner), 1);
}

// ── #199: retargeting respects the per-contract limit ────────────────────────

/// Regression test for #199: `update_target_contract` used to move an alert
/// into any contract's index without checking the per-contract limit, so
/// alerts registered against throwaway targets could all be retargeted at one
/// victim contract.
#[test]
fn test_regression_retarget_respects_per_contract_limit() {
    let (env, admin, client) = setup();
    client.set_per_contract_alert_limit(&admin, &1);

    let owner = Address::generate(&env);
    let victim = Address::generate(&env);
    let throwaway = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    client.register_alert(&owner, &victim, &str(&env, "Fills"), &hash64(&env), &rules);
    let other = client.register_alert(
        &owner,
        &throwaway,
        &str(&env, "Moves"),
        &hash64(&env),
        &rules,
    );

    assert_eq!(
        client
            .try_update_target_contract(&owner, &other, &victim)
            .unwrap_err()
            .unwrap(),
        ContractError::ContractAlertLimitExceeded
    );
    assert_eq!(
        client.get_alert(&owner, &other).unwrap().target_contract,
        throwaway
    );
    assert_eq!(client.get_active_contract_alert_count(&victim), 1);

    // Retargeting to the alert's current target is not blocked by its own slot.
    client.update_target_contract(&owner, &other, &throwaway);
}

// ── #200: transfers respect the recipient's per-owner limit ──────────────────

/// Regression test for #200: accepting a transfer used to push the alert into
/// the recipient's index without checking their per-owner limit, so colluding
/// accounts could pile any number of alerts onto one owner.
#[test]
fn test_regression_transfer_respects_recipient_per_owner_limit() {
    let (env, admin, client) = setup();
    client.set_per_owner_alert_limit(&admin, &1);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];
    let target = Address::generate(&env);

    client.register_alert(
        &recipient,
        &target,
        &str(&env, "Own"),
        &hash64(&env),
        &rules,
    );
    let gift = client.register_alert(&sender, &target, &str(&env, "Gift"), &hash64(&env), &rules);

    client.propose_alert_transfer(&sender, &gift, &recipient);
    assert_eq!(
        client
            .try_accept_alert_transfer(&recipient, &gift)
            .unwrap_err()
            .unwrap(),
        ContractError::OwnerAlertLimitExceeded
    );
    assert_eq!(client.get_alert(&sender, &gift).unwrap().owner, sender);
    assert_eq!(client.get_non_removed_alert_count(&recipient), 1);

    // Once the recipient frees a slot, the same proposal can be accepted.
    let own = client.get_alert_ids_by_owner(&recipient).get(0).unwrap();
    client.remove_alert(&recipient, &own);
    client.accept_alert_transfer(&recipient, &gift);
    assert_eq!(client.get_alert(&sender, &gift).unwrap().owner, recipient);
}

// ── #202: owners cannot undo an admin deactivation ───────────────────────────

/// Regression test for #202: `deactivate_alert_by_admin` is for moderation,
/// but the owner could immediately undo it with `update_alert(.., true)`.
/// The alert is now suspended until an admin unlocks it.
#[test]
fn test_regression_owner_cannot_undo_admin_deactivation() {
    let (env, admin, client) = setup();
    let owner = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];
    let id = client.register_alert(
        &owner,
        &Address::generate(&env),
        &str(&env, "Spam"),
        &hash64(&env),
        &rules,
    );

    client.deactivate_alert_by_admin(&admin, &id);
    assert!(client.is_alert_suspended(&id));
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &rules, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertSuspended
    );
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));

    // The owner may still edit the alert as long as it stays inactive.
    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false);

    // Only the admin can lift the suspension; lifting it does not reactivate.
    assert_eq!(
        client
            .try_unlock_alert_by_admin(&owner, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    client.unlock_alert_by_admin(&admin, &id);
    assert!(!client.is_alert_suspended(&id));
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));

    client.update_alert(&owner, &id, &rules, &true);
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));

    // Unlocking an alert that is not suspended is a no-op.
    client.unlock_alert_by_admin(&admin, &id);
}

#[test]
fn test_regression_admin_suspension_follows_the_alert() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];
    let id = client.register_alert(
        &owner,
        &Address::generate(&env),
        &str(&env, "Spam"),
        &hash64(&env),
        &rules,
    );
    client.deactivate_alert_by_admin(&admin, &id);

    // Handing the alert to another account does not escape the suspension.
    client.propose_alert_transfer(&owner, &id, &new_owner);
    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(
        client
            .try_update_alert(&new_owner, &id, &rules, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertSuspended
    );

    // Removing the alert clears the flag with it.
    client.remove_alert(&new_owner, &id);
    assert!(!client.is_alert_suspended(&id));
}

/// Regression test for #204:
/// `deactivate_all_alerts` returned a plain `0` while paused, which callers
/// could not tell apart from "owner had no active alerts". It now returns
/// `Err(Paused)` like every other mutator and leaves the alerts untouched.
#[test]
fn test_regression_deactivate_all_alerts_rejects_while_paused() {
    let (env, admin, client) = setup();
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

/// A paused watcher-registry update must fail before probing the supplied
/// address, so a malformed address cannot trap or replace `Paused`.
#[test]
fn test_regression_set_watcher_registry_checks_pause_before_probe() {
    let (env, admin, client) = setup();
    let invalid_registry = Address::generate(&env);
    client.pause(&admin);

    assert_eq!(
        client
            .try_set_watcher_registry(&admin, &invalid_registry)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );
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
    let (env, _admin, client) = setup();
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
    let (env, admin, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

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
    let (env, admin, client) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];
    let id = client.register_alert(
        &owner,
        &Address::generate(&env),
        &str(&env, "Spam"),
        &hash64(&env),
        &rules,
    );
    client.deactivate_alert_by_admin(&admin, &id);

    // Handing the alert to another account does not escape the suspension.
    client.propose_alert_transfer(&owner, &id, &new_owner);
    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(
        client
            .try_update_alert(&new_owner, &id, &rules, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertSuspended
    );

    // Removing the alert clears the flag with it.
    client.remove_alert(&new_owner, &id);
    assert!(!client.is_alert_suspended(&id));
}

// ── #203: pause freezes every state-mutating entry point ─────────────────────

/// Regression test for #203 (table-driven): `pause` is documented as freezing
/// every state-mutating call, but several mutators skipped the check. Call
/// each one while paused and expect `Paused`; the documented exemptions
/// (`pause`, `unpause`, `initialize`, `upgrade`) are not in the table.
#[test]
fn test_regression_every_mutator_rejects_while_paused() {
    let (env, admin, client) = setup();
    let owner = Address::generate(&env);
    let recipient = Address::generate(&env);
    let target = Address::generate(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &rules);
    let transferring =
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &rules);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));
    client.propose_alert_transfer(&owner, &transferring, &recipient);

    client.pause(&admin);

    let paused = ContractError::Paused;
    let checks: [(&str, ContractError); 25] = [
        (
            "transfer_admin",
            client
                .try_transfer_admin(&admin, &recipient)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "set_per_owner_alert_limit",
            client
                .try_set_per_owner_alert_limit(&admin, &5)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "set_per_contract_alert_limit",
            client
                .try_set_per_contract_alert_limit(&admin, &5)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "set_global_alert_limit",
            client
                .try_set_global_alert_limit(&admin, &5)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "set_watcher_registry",
            client
                .try_set_watcher_registry(&admin, &target)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "clear_watcher_registry",
            client
                .try_clear_watcher_registry(&admin)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "register_alert",
            client
                .try_register_alert(&owner, &target, &str(&env, "C"), &hash64(&env), &rules)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "batch_register_alert",
            client
                .try_batch_register_alert(&vec![
                    &env,
                    crate::AlertInput {
                        owner: owner.clone(),
                        target_contract: target.clone(),
                        label: str(&env, "D"),
                        webhook_hash: hash64(&env),
                        rules: rules.clone(),
                    },
                ])
                .unwrap_err()
                .unwrap(),
        ),
        (
            "update_alert",
            client
                .try_update_alert(&owner, &id, &rules, &false)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "update_webhook",
            client
                .try_update_webhook(&owner, &id, &hash64c(&env, 'c'))
                .unwrap_err()
                .unwrap(),
        ),
        (
            "propose_webhook",
            client
                .try_propose_webhook(&owner, &id, &hash64c(&env, 'd'))
                .unwrap_err()
                .unwrap(),
        ),
        (
            "confirm_webhook",
            client
                .try_confirm_webhook(&owner, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "cancel_webhook_proposal",
            client
                .try_cancel_webhook_proposal(&owner, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "renew_alert_ttl",
            client
                .try_renew_alert_ttl(&owner, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "update_label",
            client
                .try_update_label(&owner, &id, &str(&env, "E"))
                .unwrap_err()
                .unwrap(),
        ),
        (
            "update_target_contract",
            client
                .try_update_target_contract(&owner, &id, &Address::generate(&env))
                .unwrap_err()
                .unwrap(),
        ),
        (
            "bump_alert",
            client.try_bump_alert(&id, &100).unwrap_err().unwrap(),
        ),
        (
            "remove_alert",
            client.try_remove_alert(&owner, &id).unwrap_err().unwrap(),
        ),
        (
            "batch_remove_alert",
            client
                .try_batch_remove_alert(&owner, &vec![&env, id])
                .unwrap_err()
                .unwrap(),
        ),
        (
            "prune_expired_alerts",
            client
                .try_prune_expired_alerts(&owner)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "remove_alert_by_admin",
            client
                .try_remove_alert_by_admin(&admin, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "deactivate_alert_by_admin",
            client
                .try_deactivate_alert_by_admin(&admin, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "unlock_alert_by_admin",
            client
                .try_unlock_alert_by_admin(&admin, &id)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "propose_alert_transfer",
            client
                .try_propose_alert_transfer(&owner, &id, &recipient)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "cancel_alert_transfer",
            client
                .try_cancel_alert_transfer(&owner, &transferring)
                .unwrap_err()
                .unwrap(),
        ),
    ];
    for (name, error) in checks {
        assert_eq!(error, paused, "{name} must be blocked while paused");
    }
    for (name, error) in [
        (
            "accept_alert_transfer",
            client
                .try_accept_alert_transfer(&recipient, &transferring)
                .unwrap_err()
                .unwrap(),
        ),
        (
            "reject_alert_transfer",
            client
                .try_reject_alert_transfer(&recipient, &transferring)
                .unwrap_err()
                .unwrap(),
        ),
    ] {
        assert_eq!(error, paused, "{name} must be blocked while paused");
    }

    // Nothing changed, and the exempt unpause still works.
    assert!(client.get_alert(&owner, &id).unwrap().active);
    assert!(client.get_pending_alert_transfer(&transferring).is_some());
    client.unpause(&admin);
    client.update_label(&owner, &id, &str(&env, "After unpause"));
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
    let (env, _admin, client) = setup();
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

/// Regression test for #379:
/// `deactivate_alert_by_admin` on an already-suspended alert is a no-op that
/// does not rewrite the alert, bump `updated_ledger`, or emit `alert.admin_off`.
#[test]
fn test_regression_deactivate_alert_by_admin_is_idempotent() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Spam"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    env.ledger().with_mut(|li| {
        li.timestamp = 1_000;
        li.sequence_number = 10;
    });
    client.deactivate_alert_by_admin(&admin, &id);
    assert!(client.is_alert_suspended(&id));
    let first = client.get_alert(&owner, &id).unwrap();
    assert!(!first.active);
    assert_eq!(first.updated_at, 1_000);
    assert_eq!(first.updated_ledger, 10);
    assert_eq!(env.events().all().len(), 1);

    // Calling deactivate_alert_by_admin again while suspended is a silent no-op.
    env.ledger().with_mut(|li| {
        li.timestamp = 2_000;
        li.sequence_number = 20;
    });
    client.deactivate_alert_by_admin(&admin, &id);
    assert!(client.is_alert_suspended(&id));
    let second = client.get_alert(&owner, &id).unwrap();
    assert_eq!(second.updated_at, 1_000);
    assert_eq!(second.updated_ledger, 10);
    assert!(env.events().all().is_empty());

    // After unlocking, suspending again applies the suspension and emits an event.
    client.unlock_alert_by_admin(&admin, &id);
    env.ledger().with_mut(|li| {
        li.timestamp = 3_000;
        li.sequence_number = 30;
    });
    client.deactivate_alert_by_admin(&admin, &id);
    assert!(client.is_alert_suspended(&id));
    let third = client.get_alert(&owner, &id).unwrap();
    assert_eq!(third.updated_at, 3_000);
    assert_eq!(third.updated_ledger, 30);
    assert_eq!(env.events().all().len(), 1);
fn alert_ttl(env: &Env, client: &AlertRegistryClient, id: u64) -> u32 {
    use soroban_sdk::testutils::storage::Persistent as _;
    env.as_contract(&client.address, || {
        env.storage().persistent().get_ttl(&crate::DataKey::Alert(id))
    })
}

/// Regression test for #371: proposing a transfer extends the alert to at
/// least the transfer window, so the recipient can still accept on day 6.
#[test]
fn test_regression_alert_outlives_open_transfer_window() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let id = register(&env, &client, &owner, "A");
    assert!(alert_ttl(&env, &client, id) < crate::ALERT_TRANSFER_EXPIRY_LEDGERS);

    client.propose_alert_transfer(&owner, &id, &new_owner);
    assert!(alert_ttl(&env, &client, id) >= crate::ALERT_TRANSFER_EXPIRY_LEDGERS);

    // Day 6 (17,280 ledgers per day): the alert must still be live and acceptable.
    env.ledger().with_mut(|li| li.sequence_number += 6 * 17_280);
    assert!(alert_ttl(&env, &client, id) >= crate::ALERT_TRANSFER_EXPIRY_LEDGERS - 6 * 17_280);
    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(client.get_alert(&new_owner, &id).unwrap().owner, new_owner);
}

/// Regression test for #372: owners can renew for a chosen TTL up to MAX_TTL.
#[test]
fn test_regression_renew_alert_ttl_for_extends_to_requested_ttl() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let id = register(&env, &client, &owner, "A");

    client.renew_alert_ttl_for(&owner, &id, &crate::MAX_TTL);
    assert_eq!(alert_ttl(&env, &client, id), crate::MAX_TTL);

    assert_eq!(
        client
            .try_renew_alert_ttl_for(&Address::generate(&env), &id, &crate::MAX_TTL)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    for bad in [0, crate::DEFAULT_TTL - 1, crate::MAX_TTL + 1] {
        assert_eq!(
            client
                .try_renew_alert_ttl_for(&owner, &id, &bad)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidTtl
        );
    }
}

/// Regression test for #373: `bump_alert` rejects TTLs below DEFAULT_TTL and
/// emits no event for them.
#[test]
fn test_regression_bump_alert_rejects_ttl_below_default() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let id = register(&env, &client, &owner, "A");

    for bad in [0, 1, crate::DEFAULT_TTL - 1] {
        assert_eq!(
            client.try_bump_alert(&id, &bad).unwrap_err().unwrap(),
            ContractError::InvalidTtl
        );
        assert!(
            env.events().all().is_empty(),
            "a rejected bump must not emit alert.bump"
        );
    }
    client.bump_alert(&id, &crate::DEFAULT_TTL);
}

/// Regression test for #374: pruning examines at most MAX_PRUNE_PER_CALL
/// entries per call and resumes from a cursor, in a single pass.
#[test]
fn test_regression_prune_expired_alerts_is_bounded_and_resumable() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let total = crate::MAX_PRUNE_PER_CALL * 3 + 7;
    for _ in 0..total {
        let id = register(&env, &client, &owner, "A");
        expire_alert_record(&env, &client, id);
    }

    let mut removed_total = 0;
    let mut passes = 0;
    loop {
        let removed = client.prune_expired_alerts(&owner);
        assert!(removed <= crate::MAX_PRUNE_PER_CALL, "a pass must be bounded");
        removed_total += removed;
        passes += 1;
        if client.get_prune_cursor(&owner).is_none() {
            break;
        }
    }
    assert_eq!(removed_total, total);
    assert_eq!(passes, 4);
    assert_eq!(client.get_non_removed_alert_count(&owner), 0);
    assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 0);
/// Regression test for #378: `set_alert_active` to the current state is a
/// no-op: no storage rewrite (`updated_ledger` unchanged) and no event.
#[test]
fn test_regression_set_alert_active_is_noop_when_state_unchanged() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let id = register(&env, &client, &owner, "A");
    let before = client.get_alert(&owner, &id).unwrap();

    env.ledger().with_mut(|li| {
        li.sequence_number += 10;
        li.timestamp += 100;
    });
    client.set_alert_active(&owner, &id, &true);

    let after = client.get_alert(&owner, &id).unwrap();
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.updated_ledger, before.updated_ledger);
    assert!(
        env.events().all().is_empty(),
        "a no-op transition must not emit alert.update"
    );

    // A real transition still updates and emits.
    client.set_alert_active(&owner, &id, &false);
    let toggled = client.get_alert(&owner, &id).unwrap();
    assert!(!toggled.active);
    assert!(toggled.updated_ledger > before.updated_ledger);
}

/// Regression test for #377: the all-zero webhook hash is rejected by
/// `register_alert`, `update_webhook` and `propose_webhook`.
#[test]
fn test_regression_all_zero_webhook_hash_is_rejected() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let zero = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &Address::generate(&env),
                &str(&env, "A"),
                &zero,
                &vec![&env, str(&env, "rule:transfer")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWebhookHash
    );

    let id = register(&env, &client, &owner, "A");
    assert_eq!(
        client
            .try_update_webhook(&owner, &id, &zero)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWebhookHash
    );
    assert_eq!(
        client
            .try_propose_webhook(&owner, &id, &zero)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWebhookHash
    );
}

/// Regression test for #376: an empty `rules` list is rejected by
/// `register_alert`, `update_alert` and `batch_register_alert`.
#[test]
fn test_regression_empty_rules_are_rejected() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    assert_eq!(
        client
            .try_register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env])
            .unwrap_err()
            .unwrap(),
        ContractError::EmptyRules
    );

    let id = register(&env, &client, &owner, "A");
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env], &true)
            .unwrap_err()
            .unwrap(),
        ContractError::EmptyRules
    );

    let input = crate::AlertInput {
        owner: owner.clone(),
        target_contract: target,
        label: str(&env, "B"),
        webhook_hash: hash64(&env),
        rules: vec![&env],
    };
    assert_eq!(
        client
            .try_batch_register_alert(&vec![&env, input])
            .unwrap_err()
            .unwrap(),
        ContractError::EmptyRules
    );
fn last_admin_event(env: &Env, name: Symbol) -> soroban_sdk::Val {
    let events = env.events().all();
    let (_, _, data) = events
        .iter()
        .rev()
        .find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(env, &topics.get(0).unwrap()) == symbol_short!("admin")
                && Symbol::from_val(env, &topics.get(1).unwrap()) == name
        })
        .expect("admin event must be emitted");
    data
}

/// Regression test for #367: `accept_admin_transfer` emits `admin.transfer`
/// as `(old_admin, new_admin)`, the same shape as `transfer_admin`, and
/// `cancel_admin_transfer` emits a tuple `(admin,)`.
#[test]
fn test_regression_admin_transfer_event_payload_shapes() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin_transfer(&admin, &new_admin);
    client.cancel_admin_transfer(&admin);
    let (cancelled_by,): (Address,) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("cancel")));
    assert_eq!(cancelled_by, admin);

    client.propose_admin_transfer(&admin, &new_admin);
    client.accept_admin_transfer(&new_admin);
    let (old, new): (Address, Address) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("transfer")));
    assert_eq!((old, new), (admin.clone(), new_admin.clone()));

    let third = Address::generate(&env);
    client.transfer_admin(&new_admin, &third);
    let (old, new): (Address, Address) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("transfer")));
    assert_eq!((old, new), (new_admin, third));
}

/// Regression test for #368: the constructor and the admin-transfer paths
/// extend the instance TTL, so a fresh registry starts on the documented
/// 31-day lifetime rather than the network default.
#[test]
fn test_regression_constructor_and_admin_transfer_extend_instance_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(AlertRegistry, (admin.clone(),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    assert_eq!(instance_ttl(&env, &client), crate::INSTANCE_BUMP_AMOUNT);

    let new_admin = Address::generate(&env);
    for step in 0..3 {
        advance_ledgers(&env, crate::INSTANCE_BUMP_AMOUNT - crate::INSTANCE_BUMP_THRESHOLD + 1);
        match step {
            0 => client.propose_admin_transfer(&admin, &new_admin),
            1 => client.cancel_admin_transfer(&admin),
            _ => {
                client.propose_admin_transfer(&admin, &new_admin);
                advance_ledgers(&env, crate::INSTANCE_BUMP_AMOUNT - crate::INSTANCE_BUMP_THRESHOLD + 1);
                client.accept_admin_transfer(&new_admin);
            }
        }
        assert_eq!(instance_ttl(&env, &client), crate::INSTANCE_BUMP_AMOUNT);
    }
}

/// Regression test for #369: the pending admin transfer is readable through
/// `get_pending_admin_transfer` and `get_configuration().pending_admin`.
#[test]
fn test_regression_pending_admin_transfer_is_exposed() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(client.get_pending_admin_transfer(), None);
    assert_eq!(client.get_configuration().pending_admin, None);

    client.propose_admin_transfer(&admin, &new_admin);
    assert_eq!(client.get_pending_admin_transfer(), Some(new_admin.clone()));
    assert_eq!(client.get_configuration().pending_admin, Some(new_admin.clone()));

    client.accept_admin_transfer(&new_admin);
    assert_eq!(client.get_pending_admin_transfer(), None);
    assert_eq!(client.get_configuration().pending_admin, None);
}

/// Regression test for #370: an admin transfer proposal cannot be accepted
/// after `ADMIN_TRANSFER_EXPIRY_LEDGERS`, and is no longer reported as pending.
#[test]
fn test_regression_admin_transfer_proposal_expires() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);
    client.propose_admin_transfer(&admin, &new_admin);

    // Still acceptable on the last valid ledger.
    advance_ledgers(&env, crate::ADMIN_TRANSFER_EXPIRY_LEDGERS);
    assert_eq!(client.get_pending_admin_transfer(), Some(new_admin.clone()));

    advance_ledgers(&env, 1);
    assert_eq!(client.get_pending_admin_transfer(), None);
    assert_eq!(
        client
            .try_accept_admin_transfer(&new_admin)
            .unwrap_err()
            .unwrap(),
        ContractError::AdminTransferExpired
    );
    assert_eq!(client.get_admin(), admin);

    // The expired proposal can be replaced by a fresh one.
    client.propose_admin_transfer(&admin, &new_admin);
    client.accept_admin_transfer(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
/// Regression test for #366: `accept_admin_transfer` must be blocked while
/// paused so a pending admin cannot complete a takeover during an incident,
/// while `cancel_admin_transfer` stays available to revoke the proposal.
#[test]
fn test_regression_accept_admin_transfer_blocked_while_paused() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let pending = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin_transfer(&admin, &pending);
    client.pause(&admin);

    assert_eq!(
        client
            .try_accept_admin_transfer(&pending)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );
    assert_eq!(client.get_admin(), admin);

    client.cancel_admin_transfer(&admin);
    client.unpause(&admin);
    assert_eq!(
        client
            .try_accept_admin_transfer(&pending)
            .unwrap_err()
            .unwrap(),
        ContractError::NoPendingTransfer
    );
}

/// Regression test for #365: propose(B) -> transfer_admin(C) -> accept(B) must
/// be rejected, because `transfer_admin` clears the stale pending proposal.
#[test]
fn test_regression_transfer_admin_clears_pending_admin_transfer() {
    let (env, client) = setup();
    let admin_a = Address::generate(&env);
    let admin_b = Address::generate(&env);
    let admin_c = Address::generate(&env);
    client.initialize(&admin_a);

    client.propose_admin_transfer(&admin_a, &admin_b);
    client.transfer_admin(&admin_a, &admin_c);

    assert_eq!(
        client
            .try_accept_admin_transfer(&admin_b)
            .unwrap_err()
            .unwrap(),
        ContractError::NoPendingTransfer
    );
    assert_eq!(client.get_admin(), admin_c);
}

/// Regression test for #364 / fix for #580: `deactivate_all_alerts` must
/// not scan beyond one window per call.  Previously the loop kept advancing
/// windows until it found an active alert, which could scan the whole index.
/// Now each call covers exactly one `MAX_DEACTIVATIONS_PER_CALL` window and
/// returns the cursor position, never `0` until the scan is complete.
#[test]
fn test_regression_deactivate_all_alerts_does_not_return_zero_mid_scan() {
    let (env, client) = setup();
    let owner = Address::generate(&env);

    let mut ids = std::vec::Vec::new();
    for _ in 0..(crate::MAX_DEACTIVATIONS_PER_CALL + 2) {
        ids.push(register(&env, &client, &owner, "A"));
    }
    // The first full window of alerts is already inactive.
    for id in ids.iter().take(crate::MAX_DEACTIVATIONS_PER_CALL as usize) {
        client.update_alert(&owner, id, &vec![&env], &false);
    }

    // First call: processes the inactive window, returns the cursor position
    // MAX_DEACTIVATIONS_PER_CALL (not 0, and not the count of deactivations).
    let cursor_after_first = client.deactivate_all_alerts(&owner);
    assert_eq!(
        cursor_after_first,
        crate::MAX_DEACTIVATIONS_PER_CALL,
        "first call must return cursor position, not deactivated count"
    );
    // The 2 active alerts in the second window are untouched so far.
    assert_eq!(client.get_active_alert_count(&owner), 2);

    // Second call: processes the remaining 2 active alerts, reaches the end.
    let cursor_after_second = client.deactivate_all_alerts(&owner);
    let total = crate::MAX_DEACTIVATIONS_PER_CALL + 2;
    assert_eq!(
        cursor_after_second, total,
        "second call must return end-of-index position"
    );
    assert_eq!(client.get_active_alert_count(&owner), 0);

    // Third call: index is fully scanned, cursor returns 0 (empty index end).
    assert_eq!(client.deactivate_all_alerts(&owner), total);
}

/// Regression test for #605: `alert.snooze` and `alert.tags` used to emit
/// `(id, until_ledger)` / `(id, count)` without the caller, unlike every other
/// owner mutation event. Both now carry the caller, and `renew_alert_ttl_for`
/// emits the same `alert.renew` payload as `renew_alert_ttl` including the
/// requested TTL. The payloads are versioned under `EVENT_SCHEMA_VERSION`.
#[test]
fn test_regression_snooze_tags_and_renew_events_include_caller_and_ttl() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let id = register(&env, &client, &owner, "A");

    let last_alert_event = |name: Symbol| -> soroban_sdk::Val {
        let events = env.events().all();
        let (_, _, data) = events
            .iter()
            .rev()
            .find(|(_, topics, _)| {
                topics.len() == 2
                    && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("alert")
                    && Symbol::from_val(&env, &topics.get(1).unwrap()) == name
            })
            .expect("alert event must be emitted");
        data
    };

    // alert.snooze carries (id, caller, until_ledger, version).
    let until_ledger = env.ledger().sequence() + 100;
    client.snooze(&owner, &id, &until_ledger);
    let (event_id, caller, until, version): (u64, Address, u32, u32) =
        FromVal::from_val(&env, &last_alert_event(symbol_short!("snooze")));
    assert_eq!(event_id, id);
    assert_eq!(caller, owner);
    assert_eq!(until, until_ledger);
    assert_eq!(version, crate::EVENT_SCHEMA_VERSION);

    // alert.tags carries (id, caller, count, version).
    client.set_tags(&owner, &id, &vec![&env, str(&env, "tag-a"), str(&env, "tag-b")]);
    let (event_id, caller, count, version): (u64, Address, u32, u32) =
        FromVal::from_val(&env, &last_alert_event(symbol_short!("tags")));
    assert_eq!(event_id, id);
    assert_eq!(caller, owner);
    assert_eq!(count, 2);
    assert_eq!(version, crate::EVENT_SCHEMA_VERSION);

    // alert.renew from renew_alert_ttl_for carries the requested TTL.
    client.renew_alert_ttl_for(&owner, &id, &crate::MAX_TTL);
    let (event_id, caller, ttl, version): (u64, Address, u32, u32) =
        FromVal::from_val(&env, &last_alert_event(symbol_short!("renew")));
    assert_eq!(event_id, id);
    assert_eq!(caller, owner);
    assert_eq!(ttl, crate::MAX_TTL);
    assert_eq!(version, crate::EVENT_SCHEMA_VERSION);

    // renew_alert_ttl emits the same shape with the default TTL.
    client.renew_alert_ttl(&owner, &id);
    let (event_id, caller, ttl, version): (u64, Address, u32, u32) =
        FromVal::from_val(&env, &last_alert_event(symbol_short!("renew")));
    assert_eq!(event_id, id);
    assert_eq!(caller, owner);
    assert_eq!(ttl, crate::DEFAULT_TTL);
    assert_eq!(version, crate::EVENT_SCHEMA_VERSION);
/// Regression test for #604: config-change events carry the previous value
/// alongside the new one (and a schema version), so auditors do not have to
/// replay every earlier event to learn what changed. The exact payload shape
/// is pinned here so a future change to the tuple is caught.
#[test]
fn test_regression_config_change_events_include_old_value() {
    let (env, admin, client) = setup();
    let registry = Address::generate(&env);

    // admin.limit: (schema_version, old, new)
    client.set_per_owner_alert_limit(&admin, &10);
    client.set_per_owner_alert_limit(&admin, &25);
    let (version, old, new): (u32, u32, u32) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("limit")));
    assert_eq!((version, old, new), (crate::EVENT_SCHEMA_VERSION, 10, 25));

    // admin.timelock: (schema_version, old, new)
    client.set_timelock(&admin, &100);
    client.set_timelock(&admin, &250);
    let (version, old, new): (u32, u32, u32) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("timelock")));
    assert_eq!((version, old, new), (crate::EVENT_SCHEMA_VERSION, 100, 250));

    // admin.watchreg: (schema_version, old, new)
    client.set_watcher_registry(&admin, &registry);
    let (version, old, new): (u32, Option<Address>, Option<Address>) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("watchreg")));
    assert_eq!(
        (version, old, new),
        (crate::EVENT_SCHEMA_VERSION, None, Some(registry.clone()))
    );

    client.clear_watcher_registry(&admin);
    let (version, old, new): (u32, Option<Address>, Option<Address>) =
        FromVal::from_val(&env, &last_admin_event(&env, symbol_short!("watchreg")));
    assert_eq!(
        (version, old, new),
        (crate::EVENT_SCHEMA_VERSION, Some(registry), None)
    );
}

/// Regression test for #363: removing an ID that is absent from the contract
/// index must not decrement `ContractLiveCount`.
#[test]
fn test_regression_remove_from_contract_index_keeps_count_when_id_absent() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let reg = |label: &str| {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env, str(&env, "rule:transfer")],
        )
    };
    let id1 = reg("one");
    let _id2 = reg("two");

    // Simulate an archived / rebuilt index that no longer lists `id1`.
    env.as_contract(&client.address, || {
        env.storage().persistent().set(
            &crate::DataKey::ContractIndex(target.clone()),
            &soroban_sdk::Vec::<u64>::new(&env),
        );
    });
    let before: u32 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get(&crate::DataKey::ContractLiveCount(target.clone()))
            .unwrap()
    });

    client.remove_alert(&owner, &id1);

    let after: u32 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get(&crate::DataKey::ContractLiveCount(target.clone()))
            .unwrap()
    });
    assert_eq!(before, after, "live count must not drift when the ID was absent");
}
