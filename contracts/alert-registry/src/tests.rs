use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Env, FromVal, String, Symbol, Vec,
};

fn setup() -> (Env, AlertRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    // Deployed through the constructor — the only init path since soroban-sdk 25.
    let contract_id = env.register(AlertRegistry, (admin.clone(),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client, admin)
}

/// A webhook hash (32-byte SHA-256 digest) with every byte set to `c`;
/// vary `c` when a test needs two hashes that must differ.
fn hash64c(env: &Env, c: char) -> soroban_sdk::BytesN<32> {
    soroban_sdk::BytesN::from_array(env, &[c as u8; 32])
}

/// The default webhook hash used by tests.
fn hash64(env: &Env) -> soroban_sdk::BytesN<32> {
    hash64c(env, '0')
}

fn str(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

/// Build a Soroban String of `n` repetitions of ASCII char `ch`.
/// Uses a fixed 8192-byte stack buffer — sufficient for the Soroban max.
fn str_repeat(env: &Env, ch: char, n: usize) -> String {
    assert!(n <= 8192, "str_repeat: n exceeds Soroban String max");
    let byte = ch as u8;
    let mut buf = [0u8; 8192];
    for b in buf.iter_mut().take(n) {
        *b = byte;
    }
    let s = core::str::from_utf8(&buf[..n]).unwrap();
    String::from_str(env, s)
}

// 1. Happy path — register and retrieve
#[test]
fn test_register_and_get_alert() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "My Alert"),
        &hash64c(&env, '4'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.label, str(&env, "My Alert"));
    assert_eq!(cfg.owner, owner);
    assert!(cfg.active);
}

// 2. Happy path — update alert
#[test]
fn test_update_alert() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false)
            .unwrap(),
        Ok(())
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(!cfg.active);
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:mint"));
}

// update_alert emits an alert.update event
#[test]
fn test_update_alert_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false);

    assert!(!crate::emitted_events(&env).is_empty());
}

// 3. Happy path — remove alert
#[test]
fn test_remove_alert() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.try_remove_alert(&owner, &id).unwrap(), Ok(()));
    assert!(client.get_alert(&owner, &id).is_none());
}

// 4. Unauthorized update rejected
#[test]
fn test_update_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_update_alert(&attacker, &id, &vec![&env], &false)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_register_alert_rejects_invalid_rules() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:unknown")],
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_update_alert_rejects_invalid_rules() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:bogus")], &true);
}

#[test]
fn test_admin_remove_any_alert() {
    let (env, client, admin) = setup();

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:mint")],
    );

    client.remove_alert_by_admin(&admin, &id);
    assert!(client.get_alert(&owner, &id).is_none());
}

// initialize emits an admin.init event on first initialization
#[test]
fn test_initialize_emits_event() {
    let (env, _client, _admin) = setup();

    assert!(!crate::emitted_events(&env).is_empty());
}

// set_per_owner_alert_limit emits an admin.limit event
#[test]
fn test_set_per_owner_alert_limit_emits_event() {
    let (env, client, admin) = setup();

    client.set_per_owner_alert_limit(&admin, &5u32);

    assert!(!crate::emitted_events(&env).is_empty());
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_admin_set_per_owner_alert_limit() {
    let (env, client, admin) = setup();
    client.set_per_owner_alert_limit(&admin, &1u32);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );
}

#[test]
fn test_admin_transfer_admin() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.remove_alert_by_admin(&new_admin, &id);
}

#[test]
fn test_upgrade_unauthorized() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);
    let wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client
            .try_upgrade(&attacker, &wasm_hash)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn test_upgrade_rejects_non_admin() {
    // The contract is deployed through `__constructor(admin)`, so an
    // uninitialised registry no longer exists — the admin is set at
    // deployment, and `NotInitialized` is unreachable here. What still has to
    // hold is that only the deployment admin can upgrade.
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);
    let wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client
            .try_upgrade(&caller, &wasm_hash)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// ── Pause / circuit-breaker tests ────────────────────────────────────────

#[test]
fn test_pause_blocks_mutations() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    let result = client.try_register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert_eq!(result.unwrap_err().unwrap(), ContractError::Paused);
}

#[test]
fn test_unpause_restores_mutations() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.pause(&admin);
    client.unpause(&admin);
    assert!(!client.is_paused());

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert!(client.get_alert(&owner, &id).is_some());
}

#[test]
fn test_pause_allows_reads() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    client.pause(&admin);

    assert!(client.get_alert(&owner, &id).is_some());
    assert_eq!(client.get_alert_count(), 1);
}

#[test]
fn test_pause_unauthorized() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    let result = client.try_pause(&attacker);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::Unauthorized);
}

// 5. Unauthorized remove rejected
#[test]
fn test_remove_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_remove_alert(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 6. Edge case — get nonexistent alert returns None
#[test]
fn test_get_nonexistent_alert() {
    let (env, client, _admin) = setup();
    assert!(client
        .get_alert(&Address::generate(&env), &999u64)
        .is_none());
}

// 7. Edge case — get alerts for contract with no alerts returns empty vec
#[test]
fn test_get_alerts_for_contract_empty() {
    let (env, client, _admin) = setup();
    let querier = Address::generate(&env);
    let target = Address::generate(&env);
    assert_eq!(client.get_alerts_for_contract(&querier, &target).len(), 0);
}

// 8. Index queries
#[test]
fn test_index_queries() {
    let (env, client, _admin) = setup();
    let querier = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64c(&env, '1'),
        &vec![&env],
    );
    client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64c(&env, '2'),
        &vec![&env],
    );

    assert_eq!(client.get_alerts_for_contract(&querier, &target).len(), 2);
    assert_eq!(client.get_alerts_by_owner(&querier, &owner).len(), 2);
}

// 9. get_alert_count is monotonic
#[test]
fn test_get_alert_count() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    assert_eq!(client.get_alert_count(), 0);
    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 1);
    client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 2);
    client.remove_alert(&owner, &id);
    assert_eq!(client.get_alert_count(), 2);
}

// get_active_alert_count decreases after remove
#[test]
fn test_get_active_alert_count() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    assert_eq!(client.get_active_alert_count(&owner), 0);
    let id1 = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    let _id2 = client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_active_alert_count(&owner), 2);
    client.remove_alert(&owner, &id1);
    assert_eq!(client.get_active_alert_count(&owner), 1);
}

// get_active_alert_count filters out deactivated-but-not-removed alerts,
// while get_non_removed_alert_count counts every live alert regardless of
// the active flag.
#[test]
fn test_get_active_alert_count_excludes_deactivated() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    let id2 = client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
    let id3 = client.register_alert(&owner, &target, &str(&env, "C"), &hash64(&env), &vec![&env]);

    assert_eq!(client.get_active_alert_count(&owner), 3);
    assert_eq!(client.get_non_removed_alert_count(&owner), 3);

    // Deactivate alert 2 — it is no longer active but still lives in storage.
    client.update_alert(&owner, &id2, &vec![&env], &false);
    assert_eq!(client.get_active_alert_count(&owner), 2);
    assert_eq!(client.get_non_removed_alert_count(&owner), 3);

    // Reactivating brings it back into the active count.
    client.update_alert(&owner, &id2, &vec![&env], &true);
    assert_eq!(client.get_active_alert_count(&owner), 3);
    assert_eq!(client.get_non_removed_alert_count(&owner), 3);

    // Deactivate again, then remove. The alert was inactive when removed, so
    // the active count is unaffected while the non-removed count drops.
    client.update_alert(&owner, &id2, &vec![&env], &false);
    client.remove_alert(&owner, &id2);
    assert_eq!(client.get_active_alert_count(&owner), 2);
    assert_eq!(client.get_non_removed_alert_count(&owner), 2);

    // Removing an active alert drops both counts.
    client.remove_alert(&owner, &id1);
    assert_eq!(client.get_active_alert_count(&owner), 1);
    assert_eq!(client.get_non_removed_alert_count(&owner), 1);

    let _ = id3;
}

// 10. update_webhook changes the hash
#[test]
fn test_update_webhook() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64c(&env, 'a'),
        &vec![&env],
    );
    assert_eq!(
        client
            .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        client.get_alert(&owner, &id).unwrap().webhook_hash,
        hash64c(&env, 'b')
    );
}

// update_webhook emits an alert.webhook event
#[test]
fn test_update_webhook_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64c(&env, 'a'),
        &vec![&env],
    );

    client.update_webhook(&owner, &id, &hash64c(&env, 'b'));

    assert!(!crate::emitted_events(&env).is_empty());
}

// 11. update_webhook unauthorized
#[test]
fn test_update_webhook_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    assert_eq!(
        client
            .try_update_webhook(&attacker, &id, &hash64c(&env, 'e'))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn test_update_alert_missing_returns_not_found() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    assert_eq!(
        client
            .try_update_alert(&attacker, &999u64, &vec![&env], &false)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

#[test]
fn test_remove_alert_nonexistent_returns_not_found() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    assert_eq!(
        client
            .try_remove_alert(&owner, &999u64)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 12. active defaults to true on registration
#[test]
fn test_active_defaults_to_true() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );
    assert!(client.get_alert(&owner, &id).unwrap().active);
}

// 13. register_alert rejects more than 50 rules
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_register_alert_too_many_rules() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut rules: Vec<String> = vec![&env];
    for _ in 0..51u32 {
        rules.push_back(str(&env, "rule"));
    }
    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &rules);
}

// 14. update_alert rejects more than 50 rules
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_update_alert_too_many_rules() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);

    let mut rules: Vec<String> = vec![&env];
    for _ in 0..51u32 {
        rules.push_back(str(&env, "rule"));
    }
    client.update_alert(&owner, &id, &rules, &true);
}

// 15. exactly 50 rules is accepted
#[test]
fn test_register_alert_exactly_50_rules() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Only two rule descriptors exist and each may appear at most once, so a
    // 50-entry list can only be built out of duplicates — which the duplicate
    // check rejects. The 50 entries are still < the 51-entry `TooManyRules`
    // boundary, so the duplicate rule is what has to fire here.
    let mut rules: Vec<String> = vec![&env];
    for i in 0..50u32 {
        rules.push_back(str(
            &env,
            if i % 2 == 0 {
                "rule:transfer"
            } else {
                "rule:mint"
            },
        ));
    }
    assert_eq!(
        client
            .try_register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &rules)
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateRule
    );

    // 51 entries are rejected by the length check instead.
    let mut too_many: Vec<String> = vec![&env];
    for _ in 0..51u32 {
        too_many.push_back(str(&env, "rule:transfer"));
    }
    assert_eq!(
        client
            .try_register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &too_many)
            .unwrap_err()
            .unwrap(),
        ContractError::TooManyRules
    );
}

// 16. Label exceeding 128 bytes is rejected
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_label_too_long() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let long_label = str(&env, &"a".repeat(129));
    client.register_alert(&owner, &target, &long_label, &hash64(&env), &vec![&env]);
}

// 17. Label at exactly 128 bytes is accepted
#[test]
fn test_label_max_length_accepted() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let max_label = str(&env, &"a".repeat(128));
    client.register_alert(&owner, &target, &max_label, &hash64(&env), &vec![&env]);
}

// ── Soroban string-length boundary tests ─────────────────────────────────────
//
// Soroban's String type supports up to 8 192 bytes.  The contract enforces
// its own tighter 128-byte limit on `label`, so any string longer than 128
// bytes must be rejected by the contract guard long before the Soroban
// limit is reached.

// 18. Label of 8 192 bytes (Soroban max) is rejected by the app guard.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_label_at_soroban_max_rejected_by_app_guard() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let label = str_repeat(&env, 'a', 8192);
    client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
}

// 19. Label of 8 191 bytes (one below Soroban max) is also rejected by the app guard.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_label_one_below_soroban_max_rejected_by_app_guard() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let label = str_repeat(&env, 'b', 8191);
    client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
}

// 20. A Soroban String of exactly 8 192 bytes can be constructed without panicking.
#[test]
fn test_soroban_string_8192_bytes_is_constructible() {
    let (env, _client, _admin) = setup();
    let s = str_repeat(&env, 'x', 8192);
    assert_eq!(s.len(), 8192);
}

// ── Feature: renew_alert_ttl ──────────────────────────────────────────────────

// renew_alert_ttl — happy path: owner can renew without changing data
#[test]
fn test_renew_alert_ttl_happy_path() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let before = client.get_alert(&owner, &id).unwrap();

    // Advance time — renew should NOT change updated_at
    env.ledger().with_mut(|li| li.timestamp += 100);

    assert_eq!(client.try_renew_alert_ttl(&owner, &id).unwrap(), Ok(()));

    let after = client.get_alert(&owner, &id).unwrap();

    // Data must be completely unchanged
    assert_eq!(after.label, before.label);
    assert_eq!(after.webhook_hash, before.webhook_hash);
    assert_eq!(after.rules, before.rules);
    assert_eq!(after.active, before.active);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.updated_ledger, before.updated_ledger);
    assert_eq!(after.created_at, before.created_at);
}

// renew_alert_ttl emits a renew event
#[test]
fn test_renew_alert_ttl_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.renew_alert_ttl(&owner, &id);

    assert!(!crate::emitted_events(&env).is_empty());
}

// renew_alert_ttl — unauthorized caller is rejected
#[test]
fn test_renew_alert_ttl_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_renew_alert_ttl(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// renew_alert_ttl — nonexistent alert returns AlertNotFound
#[test]
fn test_renew_alert_ttl_not_found() {
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);

    assert_eq!(
        client
            .try_renew_alert_ttl(&caller, &999u64)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// ── Feature: propose_webhook / confirm_webhook ────────────────────────────────

// propose_webhook — happy path: pending hash is stored, live hash unchanged
#[test]
fn test_propose_webhook_stores_pending_hash() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64c(&env, 'a'),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_propose_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap(),
        Ok(())
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    // Live hash must still be the original
    assert_eq!(cfg.webhook_hash, hash64c(&env, 'a'));
    // Pending hash must be set
    assert_eq!(cfg.pending_webhook_hash, Some(hash64c(&env, 'b')));
}

// confirm_webhook — happy path: pending hash is promoted to live hash
#[test]
fn test_confirm_webhook_promotes_pending_hash() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64c(&env, 'a'),
        &vec![&env],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));

    assert_eq!(client.try_confirm_webhook(&owner, &id).unwrap(), Ok(()));

    let cfg = client.get_alert(&owner, &id).unwrap();
    // Live hash must now be the new one
    assert_eq!(cfg.webhook_hash, hash64c(&env, 'b'));
    // Pending hash must be cleared
    assert!(cfg.pending_webhook_hash.is_none());
}

// confirm_webhook — returns NoPendingWebhook when no rotation is in progress
#[test]
fn test_confirm_webhook_no_pending_returns_error() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_confirm_webhook(&owner, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NoPendingWebhook
    );
}

// propose_webhook — unauthorized caller is rejected
#[test]
fn test_propose_webhook_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_propose_webhook(&attacker, &id, &hash64c(&env, 'e'))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// confirm_webhook — unauthorized caller is rejected
#[test]
fn test_confirm_webhook_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));

    assert_eq!(
        client
            .try_confirm_webhook(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// propose_webhook — nonexistent alert returns AlertNotFound
#[test]
fn test_propose_webhook_not_found() {
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);

    assert_eq!(
        client
            .try_propose_webhook(&caller, &999u64, &hash64(&env))
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// confirm_webhook — nonexistent alert returns AlertNotFound
#[test]
fn test_confirm_webhook_not_found() {
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);

    assert_eq!(
        client
            .try_confirm_webhook(&caller, &999u64)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// propose_webhook — calling propose twice overwrites the pending hash
#[test]
fn test_propose_webhook_overwrites_previous_pending() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64c(&env, 's'),
        &vec![&env],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 't'));
    client.propose_webhook(&owner, &id, &hash64c(&env, 'u'));

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.pending_webhook_hash, Some(hash64c(&env, 'u')));
    // Live hash still unchanged
    assert_eq!(cfg.webhook_hash, hash64c(&env, 's'));
}

// Full rotation flow: propose → confirm → propose again → confirm again
#[test]
fn test_webhook_rotation_full_cycle() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64c(&env, 'p'),
        &vec![&env],
    );

    // First rotation
    client.propose_webhook(&owner, &id, &hash64c(&env, 'q'));
    client.confirm_webhook(&owner, &id);
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.webhook_hash, hash64c(&env, 'q'));
    assert!(cfg.pending_webhook_hash.is_none());

    // Second rotation
    client.propose_webhook(&owner, &id, &hash64c(&env, 'r'));
    client.confirm_webhook(&owner, &id);
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.webhook_hash, hash64c(&env, 'r'));
    assert!(cfg.pending_webhook_hash.is_none());
}

// propose_webhook emits a wh_prop event
#[test]
fn test_propose_webhook_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));

    assert!(!crate::emitted_events(&env).is_empty());
}

// confirm_webhook emits a wh_conf event
#[test]
fn test_confirm_webhook_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.propose_webhook(&owner, &id, &hash64c(&env, 'b'));
    client.confirm_webhook(&owner, &id);

    assert!(!crate::emitted_events(&env).is_empty());
}

// update_label emits a label event
#[test]
fn test_update_label_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_label(&owner, &id, &str(&env, "New Label"));

    assert!(!crate::emitted_events(&env).is_empty());
}

// pending_webhook_hash is None on fresh registration
#[test]
fn test_pending_webhook_hash_none_on_registration() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(cfg.pending_webhook_hash.is_none());
}

// #64 — 10 alerts from the same owner watching the same contract
//
// Registers 10 alerts from a single owner all targeting the same contract.
// Verifies that both the OwnerIndex and the ContractIndex contain exactly
// 10 entries after all registrations.
#[test]
fn test_ten_alerts_same_owner_same_contract() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let webhook_hash = hash64c(&env, 'a');
    // 10 distinct labels
    let labels = [
        "Alert 0", "Alert 1", "Alert 2", "Alert 3", "Alert 4", "Alert 5", "Alert 6", "Alert 7",
        "Alert 8", "Alert 9",
    ];

    for label in labels {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &webhook_hash,
            &vec![&env],
        );
    }

    // Both indexes must contain exactly 10 entries
    assert_eq!(
        client.get_alerts_by_owner(&owner, &owner).len(),
        10,
        "owner index must contain exactly 10 entries"
    );
    assert_eq!(
        client.get_alerts_for_contract(&owner, &target).len(),
        10,
        "contract index must contain exactly 10 entries"
    );
}

// deactivate_all_alerts must refresh OwnerIndex/ContractIndex TTLs, not just
// Alert(id)/AlertActive(id), even though it iterates the owner's entire index.
#[test]
fn test_deactivate_all_alerts_refreshes_owner_and_contract_index_ttl() {
    use crate::DataKey;
    use soroban_sdk::testutils::storage::Persistent;

    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::OwnerIndex(owner.clone()), 0, 0);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ContractIndex(target.clone()), 0, 0);
    });

    let count = client.deactivate_all_alerts(&owner);
    assert_eq!(count, 1);

    let owner_ttl = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::OwnerIndex(owner.clone()))
    });
    let contract_ttl = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::ContractIndex(target.clone()))
    });

    assert!(
        owner_ttl > 0,
        "OwnerIndex TTL must be refreshed by deactivate_all_alerts"
    );
    assert!(
        contract_ttl > 0,
        "ContractIndex TTL must be refreshed by deactivate_all_alerts"
    );
}

// propose_webhook/confirm_webhook must refresh OwnerIndex/ContractIndex TTLs,
// not just the Alert(id) key, so an alert that is only ever webhook-rotated
// stays reachable via get_alerts_by_owner / get_alerts_for_contract.
#[test]
fn test_webhook_rotation_refreshes_owner_and_contract_index_ttl() {
    use crate::DataKey;
    use soroban_sdk::testutils::storage::Persistent;

    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Let the owner/contract index TTLs run down close to expiry while the
    // alert is only rotated via propose_webhook/confirm_webhook.
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::OwnerIndex(owner.clone()), 0, 0);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ContractIndex(target.clone()), 0, 0);
    });

    client.propose_webhook(&owner, &id, &hash64c(&env, 'z'));
    client.confirm_webhook(&owner, &id);

    let owner_ttl = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::OwnerIndex(owner.clone()))
    });
    let contract_ttl = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::ContractIndex(target.clone()))
    });

    assert!(
        owner_ttl > 0,
        "OwnerIndex TTL must be refreshed by webhook rotation"
    );
    assert!(
        contract_ttl > 0,
        "ContractIndex TTL must be refreshed by webhook rotation"
    );

    // The alert must still be reachable via both indexes.
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 1);
    assert_eq!(client.get_alerts_for_contract(&owner, &target).len(), 1);
}

// set_watcher_registry emits an admin.watchreg event
#[test]
fn test_set_watcher_registry_emits_event() {
    use watcher_registry::{WatcherRegistry, WatcherRegistryClient};

    let (env, client, admin) = setup();

    let registry_id = env.register(WatcherRegistry, (admin.clone(),));
    let registry_client = WatcherRegistryClient::new(&env, &registry_id);
    registry_client.register_watcher(&admin, &admin);

    client.set_watcher_registry(&admin, &registry_id);

    assert!(!crate::emitted_events(&env).is_empty());
    assert_eq!(client.get_watcher_registry(), Some(registry_id));
}

#[test]
fn test_is_watcher_gating_enabled_default_false() {
    let (_env, client, _admin) = setup();
    assert!(!client.is_watcher_gating_enabled());
    assert!(client.get_watcher_registry().is_none());
}

// ── Mutation Testing Validation Tests (Killing Potential Mutants) ───────────

#[test]
fn test_per_owner_limit_exact_boundary() {
    let (env, client, admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.set_per_owner_alert_limit(&admin, &2u32);
    assert_eq!(client.get_per_owner_alert_limit(), 2u32);

    let id0 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A0"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert_eq!(client.get_active_alert_count(&owner), 2);

    // 3rd alert exceeds limit
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "A2"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:transfer")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::OwnerAlertLimitExceeded
    );

    // Remove alert 0
    client.remove_alert(&owner, &id0);
    assert_eq!(client.get_active_alert_count(&owner), 1);

    // Now 3rd alert can be registered
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    assert_eq!(client.get_active_alert_count(&owner), 2);

    let _ = (id1, id2);
}

#[test]
fn test_validate_rule_mint_and_invalid_descriptors() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // mint rule is valid
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Mint Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:mint")],
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:mint"));

    // invalid rules rejected
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Bad Alert"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:burn")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidRuleDescriptor
    );

    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Empty Rule"),
                &hash64(&env),
                &vec![&env, str(&env, "")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidRuleDescriptor
    );
}

#[test]
fn test_duplicate_rules_rejected() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Duplicate transfer rule rejected at registration.
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Dup Alert"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:transfer"), str(&env, "rule:transfer")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateRule
    );

    // Duplicate mint rule rejected at registration.
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Dup Mint"),
                &hash64(&env),
                &vec![&env, str(&env, "rule:mint"), str(&env, "rule:mint")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateRule
    );

    // The two distinct descriptors together are still accepted.
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Both Rules"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer"), str(&env, "rule:mint")],
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.rules.len(), 2);

    // Duplicates are also rejected when updating an alert's rules.
    let dup_mint = vec![&env, str(&env, "rule:mint"), str(&env, "rule:mint")];
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &dup_mint, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateRule
    );
}

#[test]
fn test_update_target_contract_moves_indices() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);
    let querier = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target_a,
        &str(&env, "Target Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alerts_for_contract(&querier, &target_a).len(), 1);
    assert_eq!(client.get_alerts_for_contract(&querier, &target_b).len(), 0);

    client.update_target_contract(&owner, &id, &target_b);

    assert_eq!(client.get_alerts_for_contract(&querier, &target_a).len(), 0);
    assert_eq!(client.get_alerts_for_contract(&querier, &target_b).len(), 1);
    assert_eq!(
        client
            .get_active_alerts_for_contract(&querier, &target_a)
            .len(),
        0
    );
    assert_eq!(
        client
            .get_active_alerts_for_contract(&querier, &target_b)
            .len(),
        1
    );
}

#[test]
fn test_update_target_contract_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target_a,
        &str(&env, "Target Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_target_contract(&owner, &id, &target_b);

    assert!(!crate::emitted_events(&env).is_empty());
}

#[test]
fn test_get_alert_active_states_and_counts() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    assert_eq!(client.get_alert_active(&owner, &999), None);
    assert_eq!(client.get_alert_count(), 0);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Active Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
    assert_eq!(client.get_alert_count(), 1);

    client.update_alert(&owner, &id, &vec![&env], &false);
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));

    client.remove_alert(&owner, &id);
    assert_eq!(client.get_alert_active(&owner, &id), None);
    assert_eq!(client.get_alert_count(), 1); // count is total allocated
}

#[test]
fn test_deactivate_all_alerts_precise_behavior() {
    let (env, client, _admin) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let target = Address::generate(&env);

    let id0 = client.register_alert(
        &owner1,
        &target,
        &str(&env, "A0"),
        &hash64(&env),
        &vec![&env],
    );
    let id1 = client.register_alert(
        &owner1,
        &target,
        &str(&env, "A1"),
        &hash64(&env),
        &vec![&env],
    );
    let id2 = client.register_alert(
        &owner1,
        &target,
        &str(&env, "A2"),
        &hash64(&env),
        &vec![&env],
    );
    let id3 = client.register_alert(
        &owner2,
        &target,
        &str(&env, "B0"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_active_alert_count(&owner1), 3);
    assert_eq!(client.get_active_alert_count(&owner2), 1);

    let count = client.deactivate_all_alerts(&owner1);
    assert!(!crate::emitted_events(&env).is_empty());
    assert_eq!(count, 3);
    assert_eq!(
        client.get_alert_active(&Address::generate(&env), &id0),
        Some(false)
    );
    assert_eq!(
        client.get_alert_active(&Address::generate(&env), &id1),
        Some(false)
    );
    assert_eq!(
        client.get_alert_active(&Address::generate(&env), &id2),
        Some(false)
    );
    assert_eq!(
        client.get_alert_active(&Address::generate(&env), &id3),
        Some(true)
    );

    // Second deactivate is a no-op
    let count2 = client.deactivate_all_alerts(&owner1);
    assert_eq!(count2, 0);
}

#[test]
fn test_get_alerts_modified_since_precision() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    let id0 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A0"),
        &hash64(&env),
        &vec![&env],
    );

    env.ledger().set_timestamp(2000);
    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64(&env),
        &vec![&env],
    );

    env.ledger().set_timestamp(3000);
    client.update_webhook(&owner, &id0, &hash64c(&env, 'z'));

    let res_0 = client.get_alerts_modified_since(&0, &0u32, &u32::MAX);
    assert_eq!(res_0.len(), 2);

    let res_1000 = client.get_alerts_modified_since(&1000, &0u32, &u32::MAX);
    assert_eq!(res_1000.len(), 2);

    let res_2000 = client.get_alerts_modified_since(&2000, &0u32, &u32::MAX);
    assert_eq!(res_2000.len(), 2);

    let res_2001 = client.get_alerts_modified_since(&2001, &0u32, &u32::MAX);
    assert_eq!(res_2001.len(), 1);
    assert_eq!(res_2001.get(0).unwrap().label, str(&env, "A0"));

    let res_3000 = client.get_alerts_modified_since(&3000, &0u32, &u32::MAX);
    assert_eq!(res_3000.len(), 1);
    assert_eq!(res_3000.get(0).unwrap().label, str(&env, "A0"));

    let res_3001 = client.get_alerts_modified_since(&3001, &0u32, &u32::MAX);
    assert_eq!(res_3001.len(), 0);

    let _ = id1;
}

#[test]
fn test_get_alerts_modified_since_ledger_precision() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Simulate multiple ledgers sharing the same close-time second (timestamp 1000)
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
        li.sequence_number = 100;
    });
    let id0 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A0"),
        &hash64(&env),
        &vec![&env],
    );

    // Next ledger closed in same second
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
        li.sequence_number = 101;
    });
    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64(&env),
        &vec![&env],
    );

    // Next ledger closed in same second
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
        li.sequence_number = 102;
    });
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64(&env),
        &vec![&env],
    );

    // Initial sequence checks
    let cfg0 = client.get_alert(&owner, &id0).unwrap();
    assert_eq!(cfg0.updated_ledger, 100);
    let cfg1 = client.get_alert(&owner, &id1).unwrap();
    assert_eq!(cfg1.updated_ledger, 101);
    let cfg2 = client.get_alert(&owner, &id2).unwrap();
    assert_eq!(cfg2.updated_ledger, 102);

    // With timestamp-based query, since=1000 returns all 3, but since=1001 returns none
    assert_eq!(
        client
            .get_alerts_modified_since(&1000, &0u32, &u32::MAX)
            .len(),
        3
    );
    assert_eq!(
        client
            .get_alerts_modified_since(&1001, &0u32, &u32::MAX)
            .len(),
        0
    );

    // Monotonic ledger-based query has no ambiguity:
    // since_ledger = 0 returns all 3
    let res_0 = client.get_alerts_modified_since_ledger(&0, &0u32, &u32::MAX);
    assert_eq!(res_0.len(), 3);

    // since_ledger = 100 returns all 3
    let res_100 = client.get_alerts_modified_since_ledger(&100, &0u32, &u32::MAX);
    assert_eq!(res_100.len(), 3);

    // since_ledger = 101 returns id1 and id2
    let res_101 = client.get_alerts_modified_since_ledger(&101, &0u32, &u32::MAX);
    assert_eq!(res_101.len(), 2);
    assert_eq!(res_101.get(0).unwrap().label, str(&env, "A1"));
    assert_eq!(res_101.get(1).unwrap().label, str(&env, "A2"));

    // since_ledger = 102 returns only id2
    let res_102 = client.get_alerts_modified_since_ledger(&102, &0u32, &u32::MAX);
    assert_eq!(res_102.len(), 1);
    assert_eq!(res_102.get(0).unwrap().label, str(&env, "A2"));

    // since_ledger = 103 returns 0
    let res_103 = client.get_alerts_modified_since_ledger(&103, &0u32, &u32::MAX);
    assert_eq!(res_103.len(), 0);

    // Now update id0 at ledger sequence 200
    env.ledger().with_mut(|li| {
        li.timestamp = 2000;
        li.sequence_number = 200;
    });
    client.update_webhook(&owner, &id0, &hash64c(&env, 'z'));

    let cfg0_after = client.get_alert(&owner, &id0).unwrap();
    assert_eq!(cfg0_after.updated_ledger, 200);

    // Now since_ledger = 105 returns only id0 (updated at ledger 200)
    let res_105 = client.get_alerts_modified_since_ledger(&105, &0u32, &u32::MAX);
    assert_eq!(res_105.len(), 1);
    assert_eq!(res_105.get(0).unwrap().label, str(&env, "A0"));

    // Pagination test: offset 0, limit 1
    let page1 = client.get_alerts_modified_since_ledger(&0, &0, &1);
    assert_eq!(page1.len(), 1);
    assert_eq!(page1.get(0).unwrap().label, str(&env, "A0"));

    let page2 = client.get_alerts_modified_since_ledger(&0, &1, &1);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap().label, str(&env, "A1"));

    // Removed alerts are excluded
    client.remove_alert(&owner, &id1);
    let res_after_remove = client.get_alerts_modified_since_ledger(&0, &0u32, &u32::MAX);
    assert_eq!(res_after_remove.len(), 2);
    assert_eq!(res_after_remove.get(0).unwrap().label, str(&env, "A0"));
    assert_eq!(res_after_remove.get(1).unwrap().label, str(&env, "A2"));
}

#[test]
fn test_updated_ledger_tracked_on_all_mutations() {
    let (env, client, admin) = setup();

    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);

    env.ledger().with_mut(|li| li.sequence_number = 10);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Test"),
        &hash64(&env),
        &vec![&env],
    );
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 10);

    // update_alert
    env.ledger().with_mut(|li| li.sequence_number = 20);
    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &true);
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 20);

    // update_label
    env.ledger().with_mut(|li| li.sequence_number = 30);
    client.update_label(&owner, &id, &str(&env, "New Label"));
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 30);

    // update_webhook
    env.ledger().with_mut(|li| li.sequence_number = 40);
    client.update_webhook(&owner, &id, &hash64c(&env, 'w'));
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 40);

    // propose_webhook (does not change updated_ledger or updated_at)
    env.ledger().with_mut(|li| li.sequence_number = 50);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 40);

    // confirm_webhook
    env.ledger().with_mut(|li| li.sequence_number = 60);
    client.confirm_webhook(&owner, &id);
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 60);

    // propose and cancel_webhook_proposal
    env.ledger().with_mut(|li| li.sequence_number = 70);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'q'));
    env.ledger().with_mut(|li| li.sequence_number = 80);
    client.cancel_webhook_proposal(&owner, &id);
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 80);

    // update_target_contract
    env.ledger().with_mut(|li| li.sequence_number = 90);
    client.update_target_contract(&owner, &id, &new_target);
    assert_eq!(client.get_alert(&owner, &id).unwrap().updated_ledger, 90);

    // transfer_alert_ownership
    env.ledger().with_mut(|li| li.sequence_number = 100);
    client.transfer_alert_ownership(&owner, &id, &new_owner);
    assert_eq!(
        client.get_alert(&new_owner, &id).unwrap().updated_ledger,
        100
    );

    // deactivate_alert_by_admin
    env.ledger().with_mut(|li| li.sequence_number = 110);
    client.deactivate_alert_by_admin(&admin, &id);
    assert_eq!(
        client.get_alert(&new_owner, &id).unwrap().updated_ledger,
        110
    );

    // deactivate_all_alerts
    env.ledger().with_mut(|li| li.sequence_number = 120);
    client.update_alert(&new_owner, &id, &vec![&env], &true);
    assert_eq!(
        client.get_alert(&new_owner, &id).unwrap().updated_ledger,
        120
    );
    env.ledger().with_mut(|li| li.sequence_number = 130);
    client.deactivate_all_alerts(&new_owner);
    assert_eq!(
        client.get_alert(&new_owner, &id).unwrap().updated_ledger,
        130
    );
}

#[test]
fn test_configs_paginated_boundaries() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let querier = Address::generate(&env);

    for i in 0..5 {
        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64(&env),
            &vec![&env],
        );
        let _ = i;
    }

    let p1 = client.get_alerts_by_owner_paginated(&querier, &owner, &0, &2);
    assert_eq!(p1.len(), 2);

    let p2 = client.get_alerts_by_owner_paginated(&querier, &owner, &2, &2);
    assert_eq!(p2.len(), 2);

    let p3 = client.get_alerts_by_owner_paginated(&querier, &owner, &4, &2);
    assert_eq!(p3.len(), 1);

    let p4 = client.get_alerts_by_owner_paginated(&querier, &owner, &5, &2);
    assert_eq!(p4.len(), 0);

    let p5 = client.get_alerts_by_owner_paginated(&querier, &owner, &10, &2);
    assert_eq!(p5.len(), 0);
}

// ── Issue #34 — transfer_alert_ownership ────────────────────────────────

#[test]
fn test_transfer_alert_ownership_success() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.transfer_alert_ownership(&owner, &id, &new_owner);

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.owner, new_owner);

    // OwnerIndex updated for both the old and new owner.
    assert_eq!(client.get_alerts_by_owner(&new_owner, &owner).len(), 0);
    let new_owner_alerts = client.get_alerts_by_owner(&new_owner, &new_owner);
    assert_eq!(new_owner_alerts.len(), 1);
    assert_eq!(new_owner_alerts.get(0).unwrap().owner, new_owner);
}

#[test]
fn test_transfer_alert_ownership_unauthorized() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_transfer_alert_ownership(&attacker, &id, &new_owner)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    // Ownership and indexes are unchanged.
    assert_eq!(client.get_alert(&owner, &id).unwrap().owner, owner);
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 1);
}

#[test]
fn test_transfer_alert_ownership_not_found() {
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);
    let new_owner = Address::generate(&env);

    assert_eq!(
        client
            .try_transfer_alert_ownership(&caller, &999u64, &new_owner)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

#[test]
fn test_transfer_alert_ownership_emits_event() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.transfer_alert_ownership(&owner, &id, &new_owner);
    assert!(!crate::emitted_events(&env).is_empty());
}

// ── Issue #36 — deactivate_alert_by_admin ───────────────────────────────

#[test]
fn test_deactivate_alert_by_admin_success() {
    let (env, client, admin) = setup();

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.deactivate_alert_by_admin(&admin, &id);

    // Record still exists (not deleted) but is inactive.
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(!cfg.active);
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));
}

#[test]
fn test_deactivate_alert_by_admin_unauthorized() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_deactivate_alert_by_admin(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    assert!(client.get_alert(&owner, &id).unwrap().active);
}

#[test]
fn test_deactivate_alert_by_admin_not_found() {
    let (_env, client, admin) = setup();

    assert_eq!(
        client
            .try_deactivate_alert_by_admin(&admin, &999u64)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

#[test]
fn test_deactivate_alert_by_admin_emits_event() {
    let (env, client, admin) = setup();

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.deactivate_alert_by_admin(&admin, &id);
    assert!(!crate::emitted_events(&env).is_empty());
}

// ── Issue #37 — batch_register_alert / batch_remove_alert ───────────────

fn alert_input(env: &Env, owner: &Address, target: &Address, label: &str) -> AlertInput {
    AlertInput {
        owner: owner.clone(),
        target_contract: target.clone(),
        label: str(env, label),
        webhook_hash: hash64(env),
        rules: vec![env],
    }
}

#[test]
fn test_batch_register_alert_single() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let inputs = vec![&env, alert_input(&env, &owner, &target, "A0")];

    let ids = client.batch_register_alert(&inputs);
    assert_eq!(ids.len(), 1);
    assert!(client.get_alert(&owner, &ids.get(0).unwrap()).is_some());
    assert_eq!(client.get_alert_count(), 1);
}

#[test]
fn test_batch_register_alert_five() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut inputs: Vec<AlertInput> = vec![&env];
    for _ in 0..5u32 {
        inputs.push_back(alert_input(&env, &owner, &target, "A"));
    }

    let ids = client.batch_register_alert(&inputs);
    assert_eq!(ids.len(), 5);
    assert_eq!(client.get_alert_count(), 5);
    assert_eq!(client.get_active_alert_count(&owner), 5);
}

#[test]
fn test_batch_register_alert_boundary_size() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // One transaction may touch at most 100 ledger entries and write at most
    // 50; a 25-alert batch measures ~112 entries / ~55 writes, so the largest
    // batch that can actually be submitted is 12.
    let mut inputs: Vec<AlertInput> = vec![&env];
    for _ in 0..12u32 {
        inputs.push_back(alert_input(&env, &owner, &target, "A"));
    }

    let ids = client.batch_register_alert(&inputs);
    assert_eq!(ids.len(), 12);
    assert_eq!(client.get_alert_count(), 12);
    assert_eq!(client.get_active_alert_count(&owner), 12);
}

#[test]
fn test_batch_register_alert_rolls_back_on_validation_error() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // A second owner: a repeated require_auth for the same address within one
    // invocation aborts before validation runs, which would mask the error.
    let other_owner = Address::generate(&env);
    let mut bad = alert_input(&env, &other_owner, &target, "Bad");
    // 129 bytes: one over the label limit.
    bad.label = str(&env, &"a".repeat(129));

    let inputs = vec![&env, alert_input(&env, &owner, &target, "Good"), bad];

    assert_eq!(
        client
            .try_batch_register_alert(&inputs)
            .unwrap_err()
            .unwrap(),
        ContractError::LabelTooLong
    );
    // The whole batch is rolled back, including the earlier valid item.
    assert_eq!(client.get_alert_count(), 0);
}

#[test]
fn test_batch_remove_alert_single() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);

    client.batch_remove_alert(&owner, &vec![&env, id]);
    assert!(client.get_alert(&owner, &id).is_none());
}

#[test]
fn test_batch_remove_alert_five() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut ids: Vec<u64> = vec![&env];
    for _ in 0..5u32 {
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        ids.push_back(id);
    }

    client.batch_remove_alert(&owner, &ids);
    for i in 0..ids.len() {
        assert!(client.get_alert(&owner, &ids.get(i).unwrap()).is_none());
    }
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

#[test]
fn test_batch_remove_alert_boundary_size() {
    let (env, client, _admin) = setup();
    // 25 registrations plus the batch exceed the default instruction budget.
    env.cost_estimate().budget().reset_unlimited();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // 25 removals in one transaction exceed the 100-entry footprint limit; 20
    // is the largest batch that fits (see `test_batch_register_alert_boundary_size`).
    let mut ids: Vec<u64> = vec![&env];
    for _ in 0..20u32 {
        let id =
            client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
        ids.push_back(id);
    }

    client.batch_remove_alert(&owner, &ids);
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

#[test]
fn test_batch_remove_alert_unauthorized_rolls_back() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "B"),
        &hash64c(&env, 'b'),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_batch_remove_alert(&attacker, &vec![&env, id1, id2])
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    // Nothing removed.
    assert!(client.get_alert(&owner, &id1).is_some());
    assert!(client.get_alert(&owner, &id2).is_some());
}

#[test]
fn test_batch_remove_alert_not_found() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);

    assert_eq!(
        client
            .try_batch_remove_alert(&owner, &vec![&env, 999u64])
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// ── Consolidated tests from lib.rs ──────────────────────────────────────

fn setup_with_watcher_registry() -> (
    Env,
    AlertRegistryClient<'static>,
    watcher_registry::WatcherRegistryClient<'static>,
    Address,
) {
    use watcher_registry::WatcherRegistry;
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let alert_id = env.register(AlertRegistry, (admin.clone(),));
    let watcher_id = env.register(WatcherRegistry, (admin.clone(),));

    let alert_client = AlertRegistryClient::new(&env, &alert_id);
    let watcher_client = watcher_registry::WatcherRegistryClient::new(&env, &watcher_id);

    (env, alert_client, watcher_client, admin)
}

#[test]
fn test_global_alert_limit_defaults_to_zero_unlimited() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_global_alert_limit(), 0u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_global_alert_limit_enforced_across_owners() {
    let (env, client, admin) = setup();
    client.set_global_alert_limit(&admin, &2u32);

    let target = Address::generate(&env);
    // Two different owners share the same global ceiling.
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    // Third registration, from yet another owner, exceeds the ceiling.
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert3"),
        &hash64c(&env, '3'),
        &vec![&env, str(&env, "rule:mint")],
    );
}

#[test]
fn test_global_alert_limit_not_decremented_by_removal() {
    let (env, client, admin) = setup();
    client.set_global_alert_limit(&admin, &1u32);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert1"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.remove_alert(&owner, &id);

    // The ceiling tracks the monotonic ever-registered count, not the
    // live count, so a freed-up slot from removal does not reopen room.
    assert_eq!(
        client
            .try_register_alert(
                &owner,
                &target,
                &str(&env, "Alert2"),
                &hash64c(&env, '2'),
                &vec![&env, str(&env, "rule:mint")],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::GlobalAlertLimitExceeded
    );
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_global_alert_limit_requires_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(AlertRegistry, (admin.clone(),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    env.mock_all_auths();
    env.set_auths(&[]);
    client.set_global_alert_limit(&admin, &5u32);
}

#[test]
fn test_set_global_alert_limit_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    assert_eq!(
        client
            .try_set_global_alert_limit(&attacker, &5u32)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn test_per_contract_alert_limit_defaults_to_zero_unlimited() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_per_contract_alert_limit(), 0u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_per_contract_alert_limit_enforced_across_owners() {
    let (env, client, admin) = setup();
    client.set_per_contract_alert_limit(&admin, &2u32);

    let target = Address::generate(&env);
    // Two different owners contribute to the same target contract.
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    // Third registration against the same target, from yet another
    // owner, exceeds the per-contract ceiling.
    client.register_alert(
        &Address::generate(&env),
        &target,
        &str(&env, "Alert3"),
        &hash64c(&env, '3'),
        &vec![&env, str(&env, "rule:mint")],
    );
}

#[test]
fn test_per_contract_alert_limit_independent_per_contract() {
    let (env, client, admin) = setup();
    client.set_per_contract_alert_limit(&admin, &1u32);

    let target_a = Address::generate(&env);
    let target_b = Address::generate(&env);

    // One alert against target_a fills its ceiling...
    client.register_alert(
        &Address::generate(&env),
        &target_a,
        &str(&env, "Alert1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // ...but target_b's own ceiling is untouched.
    client.register_alert(
        &Address::generate(&env),
        &target_b,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    assert_eq!(client.get_active_contract_alert_count(&target_a), 1u32);
    assert_eq!(client.get_active_contract_alert_count(&target_b), 1u32);
}

#[test]
fn test_per_contract_alert_limit_freed_by_removal() {
    let (env, client, admin) = setup();
    client.set_per_contract_alert_limit(&admin, &1u32);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert1"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // Unlike the global ceiling, the per-contract limit tracks currently
    // active alerts, so removing one reopens room for the target.
    client.remove_alert(&owner, &id);
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );
    assert_eq!(client.get_active_contract_alert_count(&target), 1u32);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_per_contract_alert_limit_requires_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(AlertRegistry, (admin.clone(),));
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    env.set_auths(&[]);
    client.set_per_contract_alert_limit(&admin, &5u32);
}

#[test]
fn test_set_per_contract_alert_limit_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    assert_eq!(
        client
            .try_set_per_contract_alert_limit(&attacker, &5u32)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn test_set_per_contract_alert_limit_emits_admin_limit_event() {
    use soroban_sdk::symbol_short;

    let (env, client, admin) = setup();

    client.set_per_contract_alert_limit(&admin, &7u32);

    let events = crate::emitted_events(&env);
    let limit_event = events
        .iter()
        .find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("admin")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("limit")
        })
        .expect("admin.limit event must be emitted");

    let (_, _, data) = limit_event;
    let (kind, emitted_limit): (Symbol, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(kind, symbol_short!("contract"));
    assert_eq!(emitted_limit, 7u32);
}

#[test]
fn test_old_admin_rejected_after_transfer() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);

    // first transfer succeeds
    assert_eq!(
        client.try_transfer_admin(&admin, &new_admin).unwrap(),
        Ok(())
    );

    // old admin cannot call transfer_admin again
    assert_eq!(
        client
            .try_transfer_admin(&admin, &new_admin)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// Issue #49 — get_alert_count is monotonically increasing after multiple register/remove cycles
#[test]
fn test_get_alert_count_after_multiple_cycles() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Start at 0
    assert_eq!(client.get_alert_count(), 0);

    // Cycle 1: register -> count goes to 1
    let id1 = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 1);
    // remove -> count stays at 1 (monotonic)
    client.remove_alert(&owner, &id1);
    assert_eq!(client.get_alert_count(), 1);

    // Cycle 2: register -> count goes to 2
    let id2 = client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 2);
    // remove -> count stays at 2
    client.remove_alert(&owner, &id2);
    assert_eq!(client.get_alert_count(), 2);

    // Cycle 3: register -> count goes to 3
    let id3 = client.register_alert(&owner, &target, &str(&env, "C"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 3);
    // remove -> count stays at 3
    client.remove_alert(&owner, &id3);
    assert_eq!(client.get_alert_count(), 3);

    // Final verification: after 3 cycles the counter is 3, never reset to 0
    assert_eq!(client.get_alert_count(), 3);
    // No active alerts remain
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

// Issue #68 — get_alerts_by_owner returns empty vec for address with no alerts
#[test]
fn test_get_alerts_by_owner_empty() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let querier = Address::generate(&env);
    assert_eq!(client.get_alerts_by_owner(&querier, &owner).len(), 0);
}

// 8b. get_alert_ids_by_owner — thin ID-only wrapper over the owner index (#35)
#[test]
fn test_get_alert_ids_by_owner() {
    let (env, client, _admin) = setup();
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let target = Address::generate(&env);

    assert_eq!(client.get_alert_ids_by_owner(&owner).len(), 0);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64(&env),
        &vec![&env],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64(&env),
        &vec![&env],
    );

    let owned_ids = client.get_alert_ids_by_owner(&owner);
    assert_eq!(owned_ids.len(), 2);
    assert_eq!(owned_ids.get(0).unwrap(), id1);
    assert_eq!(owned_ids.get(1).unwrap(), id2);

    // Unrelated owner still sees an empty list.
    assert_eq!(client.get_alert_ids_by_owner(&other).len(), 0);
}

// 10. Paginated queries work without watcher gating
#[test]
fn test_paginated_queries_no_gating() {
    let (env, client, _admin) = setup();
    let querier = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for i in 0..5u32 {
        let label = String::from_str(&env, "alert");
        let _ = i; // suppress unused warning
        client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
    }

    let page = client.get_contract_alerts_paginated(&querier, &target, &0u32, &3u32);
    assert_eq!(page.len(), 3);

    let page2 = client.get_alerts_by_owner_paginated(&querier, &owner, &3u32, &10u32);
    assert_eq!(page2.len(), 2);
}

// 11. No watcher registry configured — any querier can read
#[test]
fn test_no_watcher_registry_any_querier_can_read() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // No registry set — stranger can still query
    assert_eq!(client.get_alerts_for_contract(&stranger, &target).len(), 1);
}

// 12. Watcher registry configured — registered watcher can read
#[test]
#[cfg(feature = "testutils")]
fn test_watcher_registry_registered_watcher_can_read() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let watcher = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.register_watcher(&admin, &watcher);

    // Point alert registry at the watcher registry
    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Registered watcher can query
    let results = alert_client.get_alerts_for_contract(&watcher, &target);
    assert_eq!(results.len(), 1);
}

// 13. Watcher registry configured — unregistered address is rejected
#[test]
#[cfg(feature = "testutils")]
fn test_watcher_registry_unregistered_address_rejected() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Stranger (not a watcher) is rejected
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
}

// 14. Watcher registry configured — removed watcher loses access
#[test]
#[cfg(feature = "testutils")]
fn test_watcher_registry_removed_watcher_loses_access() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let watcher = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // A second watcher keeps the watcher registry's MIN_WATCHERS invariant
    // satisfiable while the first one is removed.
    let keeper = Address::generate(&env);
    watcher_client.register_watcher(&admin, &keeper);
    watcher_client.register_watcher(&admin, &watcher);

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Watcher can read before removal
    assert_eq!(
        alert_client
            .get_alerts_for_contract(&watcher, &target)
            .len(),
        1
    );

    // Remove the watcher
    watcher_client.remove_watcher(&admin, &watcher);

    // Now rejected
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&watcher, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
}

// 14b. Watcher registry configured — get_alert, get_alert_active, and
// get_active_alerts_for_contract reject a non-watcher the same way the
// other gated query functions do (#42).
#[test]
#[cfg(feature = "testutils")]
fn test_watcher_registry_get_alert_family_rejects_non_watcher() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let watcher = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.register_watcher(&admin, &watcher);

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    let id = alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Registered watcher can use all three.
    assert!(alert_client.get_alert(&watcher, &id).is_some());
    assert_eq!(alert_client.get_alert_active(&watcher, &id), Some(true));
    assert_eq!(
        alert_client
            .get_active_alerts_for_contract(&watcher, &target)
            .len(),
        1
    );

    // A stranger is rejected on all three.
    assert_eq!(
        alert_client
            .try_get_alert(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
    assert_eq!(
        alert_client
            .try_get_alert_active(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
    assert_eq!(
        alert_client
            .try_get_active_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
}

// 15. get_watcher_registry returns None before configuration
#[test]
fn test_get_watcher_registry_none_before_set() {
    let (_env, client, _admin) = setup();
    assert!(client.get_watcher_registry().is_none());
    assert!(!client.is_watcher_gating_enabled());
}

// 16. set_watcher_registry persists and get_watcher_registry returns it
#[test]
#[cfg(feature = "testutils")]
fn test_set_and_get_watcher_registry() {
    let (_env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    assert_eq!(
        alert_client.get_watcher_registry().unwrap(),
        watcher_contract_id
    );
    assert!(alert_client.is_watcher_gating_enabled());
}

// 16b. is_watcher_gating_enabled convenience getter
#[test]
#[cfg(feature = "testutils")]
fn test_is_watcher_gating_enabled() {
    let (_env, alert_client, watcher_client, admin) = setup_with_watcher_registry();
    assert!(!alert_client.is_watcher_gating_enabled());

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    assert!(alert_client.is_watcher_gating_enabled());
}

// 17. Only admin can set watcher registry
#[test]
fn test_set_watcher_registry_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);
    let fake_registry = Address::generate(&env);

    assert_eq!(
        client
            .try_set_watcher_registry(&attacker, &fake_registry)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 17b. set_watcher_registry probes the target and rejects a contract that
// doesn't implement the WatcherRegistry interface (#44)
#[test]
fn test_set_watcher_registry_rejects_invalid_contract() {
    let (env, client, admin) = setup();

    // A real, deployed contract — but not a WatcherRegistry, so it has
    // no `is_watcher_authorized` entry point for the probe to find.
    let not_a_watcher_registry = env.register(AlertRegistry, (admin.clone(),));

    assert_eq!(
        client
            .try_set_watcher_registry(&admin, &not_a_watcher_registry)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWatcherRegistry
    );
    // The rejected configuration must not have been persisted.
    assert!(client.get_watcher_registry().is_none());
    assert!(!client.is_watcher_gating_enabled());
}

// 17c. set_watcher_registry rejects a plain (non-contract) address (#44)
#[test]
fn test_set_watcher_registry_rejects_non_contract_address() {
    let (env, client, admin) = setup();

    let not_a_contract = Address::generate(&env);

    assert_eq!(
        client
            .try_set_watcher_registry(&admin, &not_a_contract)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWatcherRegistry
    );
}

// 17d. set_watcher_registry accepts a real WatcherRegistry after a prior
// misconfigured attempt was rejected (#44)
#[test]
#[cfg(feature = "testutils")]
fn test_set_watcher_registry_recovers_after_invalid_attempt() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let bogus = env.register(AlertRegistry, (admin.clone(),));
    assert_eq!(
        alert_client
            .try_set_watcher_registry(&admin, &bogus)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWatcherRegistry
    );
    assert!(alert_client.get_watcher_registry().is_none());

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert_eq!(
        alert_client.get_watcher_registry().unwrap(),
        watcher_contract_id
    );
}

// 17b. clear_watcher_registry disables gating; set_watcher_registry can
// re-enable it afterward.
#[test]
#[cfg(feature = "testutils")]
fn test_clear_watcher_registry_disables_then_reconfigure() {
    let (env, alert_client, watcher_client, admin) = setup_with_watcher_registry();

    let watcher = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.register_watcher(&admin, &watcher);

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());

    let id = alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // While a registry is configured, a non-watcher is rejected.
    assert_eq!(
        alert_client
            .try_get_alert(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );

    // Clearing the registry turns gating off: the same call now succeeds.
    alert_client.clear_watcher_registry(&admin);
    assert!(!alert_client.is_watcher_gating_enabled());
    assert!(alert_client.get_alert(&stranger, &id).is_some());

    // A registry can be configured again afterwards, and gating is back on.
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());
    assert!(alert_client.get_alert(&watcher, &id).is_some());
    assert_eq!(
        alert_client
            .try_get_alert(&stranger, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
}
