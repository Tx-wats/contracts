use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    vec, Address, BytesN, Env, FromVal, String, Symbol, Vec,
};

fn setup() -> (Env, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client)
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
    let (env, client) = setup();
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

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false);

    assert!(!env.events().all().is_empty());
}

// 3. Happy path — remove alert
#[test]
fn test_remove_alert() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:bogus")], &true);
}

#[test]
fn test_admin_remove_any_alert() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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
    let (env, client) = setup();
    let admin = Address::generate(&env);

    client.initialize(&admin);

    assert!(!env.events().all().is_empty());
}

// set_per_owner_alert_limit emits an admin.limit event
#[test]
fn test_set_per_owner_alert_limit_emits_event() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_per_owner_alert_limit(&admin, &5u32);

    assert!(!env.events().all().is_empty());
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_admin_set_per_owner_alert_limit() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
fn test_upgrade_requires_initialized_admin() {
    let (env, client) = setup();
    let caller = Address::generate(&env);
    let wasm_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client.try_upgrade(&caller, &wasm_hash).unwrap_err().unwrap(),
        ContractError::NotInitialized
    );
// ── Pause / circuit-breaker tests ────────────────────────────────────────

#[test]
fn test_pause_blocks_mutations() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    assert!(client.get_alert(&id).is_some());
}

#[test]
fn test_pause_allows_reads() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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

    assert!(client.get_alert(&id).is_some());
    assert_eq!(client.get_alert_count(), 1);
}

#[test]
fn test_pause_unauthorized() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let attacker = Address::generate(&env);

    let result = client.try_pause(&attacker);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::Unauthorized);
}

// 5. Unauthorized remove rejected
#[test]
fn test_remove_unauthorized() {
    let (env, client) = setup();
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
    let (env, client) = setup();
    assert!(client.get_alert(&Address::generate(&env), &999u64).is_none());
}

// 7. Edge case — get alerts for contract with no alerts returns empty vec
#[test]
fn test_get_alerts_for_contract_empty() {
    let (env, client) = setup();
    let querier = Address::generate(&env);
    let target = Address::generate(&env);
    assert_eq!(client.get_alerts_for_contract(&querier, &target).len(), 0);
}

// 8. Index queries
#[test]
fn test_index_queries() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

// 11. update_webhook unauthorized
#[test]
fn test_update_webhook_unauthorized() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

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
    let id = client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &rules);
    assert_eq!(client.get_alert(&owner, &id).unwrap().rules.len(), 50);
}

// 16. Label exceeding 128 bytes is rejected
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_label_too_long() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let long_label = str(&env, &"a".repeat(129));
    client.register_alert(&owner, &target, &long_label, &hash64(&env), &vec![&env]);
}

// 17. Label at exactly 128 bytes is accepted
#[test]
fn test_label_max_length_accepted() {
    let (env, client) = setup();
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let label = str_repeat(&env, 'a', 8192);
    client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
}

// 19. Label of 8 191 bytes (one below Soroban max) is also rejected by the app guard.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_label_one_below_soroban_max_rejected_by_app_guard() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let label = str_repeat(&env, 'b', 8191);
    client.register_alert(&owner, &target, &label, &hash64(&env), &vec![&env]);
}

// 20. A Soroban String of exactly 8 192 bytes can be constructed without panicking.
#[test]
fn test_soroban_string_8192_bytes_is_constructible() {
    let (env, _client) = setup();
    let s = str_repeat(&env, 'x', 8192);
    assert_eq!(s.len(), 8192);
}

// ── Feature: renew_alert_ttl ──────────────────────────────────────────────────

// renew_alert_ttl — happy path: owner can renew without changing data
#[test]
fn test_renew_alert_ttl_happy_path() {
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
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

// renew_alert_ttl — unauthorized caller is rejected
#[test]
fn test_renew_alert_ttl_unauthorized() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

// confirm_webhook emits a wh_conf event
#[test]
fn test_confirm_webhook_emits_event() {
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

// update_label emits a label event
#[test]
fn test_update_label_emits_event() {
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

// pending_webhook_hash is None on fresh registration
#[test]
fn test_pending_webhook_hash_none_on_registration() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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

    let (env, client) = setup();
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

    let (env, client) = setup();
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
        env.storage().persistent().extend_ttl(
            &DataKey::OwnerIndex(owner.clone()),
            0,
            0,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ContractIndex(target.clone()),
            0,
            0,
        );
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

    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let registry_id = env.register(WatcherRegistry, ());
    let registry_client = WatcherRegistryClient::new(&env, &registry_id);
    registry_client.initialize(&admin);
    registry_client.register_watcher(&admin, &admin);

    client.set_watcher_registry(&admin, &registry_id);

    assert!(!env.events().all().is_empty());
    assert_eq!(client.get_watcher_registry(), Some(registry_id));
}

#[test]
fn test_is_watcher_gating_enabled_default_false() {
    let (_env, client) = setup();
    assert!(!client.is_watcher_gating_enabled());
    assert!(client.get_watcher_registry().is_none());
}

// ── Mutation Testing Validation Tests (Killing Potential Mutants) ───────────

#[test]
fn test_per_owner_limit_exact_boundary() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.initialize(&admin);
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    assert_eq!(client.get_active_alerts_for_contract(&querier, &target_a).len(), 0);
    assert_eq!(client.get_active_alerts_for_contract(&querier, &target_b).len(), 1);
}

#[test]
fn test_update_target_contract_emits_event() {
    let (env, client) = setup();
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

    assert!(!env.events().all().is_empty());
}

#[test]
fn test_get_alert_active_states_and_counts() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    assert!(!env.events().all().is_empty());
    assert_eq!(count, 3);
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id0), Some(false));
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id1), Some(false));
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id2), Some(false));
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id3), Some(true));

    // Second deactivate is a no-op
    let count2 = client.deactivate_all_alerts(&owner1);
    assert_eq!(count2, 0);
}

#[test]
fn test_get_alerts_modified_since_precision() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    assert_eq!(client.get_alerts_modified_since(&1000, &0u32, &u32::MAX).len(), 3);
    assert_eq!(client.get_alerts_modified_since(&1001, &0u32, &u32::MAX).len(), 0);

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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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

    // propose_alert_transfer + accept_alert_transfer
    env.ledger().with_mut(|li| li.sequence_number = 100);
    client.propose_alert_transfer(&owner, &id, &new_owner);
    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(client.get_alert(&new_owner, &id).unwrap().updated_ledger, 100);

    // deactivate_alert_by_admin
    env.ledger().with_mut(|li| li.sequence_number = 110);
    client.deactivate_alert_by_admin(&admin, &id);
    assert_eq!(client.get_alert(&new_owner, &id).unwrap().updated_ledger, 110);

    // deactivate_all_alerts
    env.ledger().with_mut(|li| li.sequence_number = 120);
    // An admin deactivation suspends the alert (#202); lift it before the
    // owner reactivates it for the next step.
    client.unlock_alert_by_admin(&admin, &id);
    client.update_alert(&new_owner, &id, &vec![&env], &true);
    assert_eq!(client.get_alert(&new_owner, &id).unwrap().updated_ledger, 120);
    env.ledger().with_mut(|li| li.sequence_number = 130);
    client.deactivate_all_alerts(&new_owner);
    assert_eq!(client.get_alert(&new_owner, &id).unwrap().updated_ledger, 130);
}

#[test]
fn test_configs_paginated_boundaries() {
    let (env, client) = setup();
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

#[test]
fn test_paginated_queries_cap_unbounded_limits() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for i in 0..=MAX_PAGE_SIZE {
        client.register_alert(
            &owner,
            &target,
            &str(&env, "Alert"),
            &hash64c(&env, char::from(b'a' + (i % 26) as u8)),
            &vec![&env],
        );
    }

    assert_eq!(
        client
            .get_alerts_by_owner_paginated(&owner, &owner, &0, &u32::MAX)
            .len(),
        MAX_PAGE_SIZE
    );
    assert_eq!(
        client
            .get_contract_alerts_paginated(&owner, &target, &0, &u32::MAX)
            .len(),
        MAX_PAGE_SIZE
    );
    assert_eq!(
        client.get_alerts_modified_since(&0, &0, &u32::MAX).len(),
        MAX_PAGE_SIZE
    );
}

// ── Issue #34 / #201 — alert ownership transfer ────────────────────────────────

#[test]
fn test_alert_transfer_success() {
    let (env, client) = setup();
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

    client.propose_alert_transfer(&owner, &id, &new_owner);

    client.accept_alert_transfer(&new_owner, &id);

    let cfg = client.get_alert(&id).unwrap();
    assert_eq!(cfg.owner, new_owner);

    // OwnerIndex updated for both the old and new owner.
    assert_eq!(client.get_alerts_by_owner(&new_owner, &owner).len(), 0);
    let new_owner_alerts = client.get_alerts_by_owner(&new_owner, &new_owner);
    assert_eq!(new_owner_alerts.len(), 1);
    assert_eq!(new_owner_alerts.get(0).unwrap().owner, new_owner);
}

#[test]
fn test_alert_transfer_unauthorized() {
    let (env, client) = setup();
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
            .try_propose_alert_transfer(&attacker, &id, &new_owner)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    // Ownership and indexes are unchanged.
    assert_eq!(client.get_alert(&id).unwrap().owner, owner);
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 1);
}

#[test]
fn test_alert_transfer_not_found() {
    let (env, client) = setup();
    let caller = Address::generate(&env);
    let new_owner = Address::generate(&env);

    assert_eq!(
        client
            .try_propose_alert_transfer(&caller, &999u64, &new_owner)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

#[test]
fn test_alert_transfer_emits_event() {
    let (env, client) = setup();
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

    client.propose_alert_transfer(&owner, &id, &new_owner);

    client.accept_alert_transfer(&new_owner, &id);
    assert!(!env.events().all().is_empty());
}

// ── Issue #36 — deactivate_alert_by_admin ───────────────────────────────

#[test]
fn test_deactivate_alert_by_admin_success() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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
    let cfg = client.get_alert(&id).unwrap();
    assert!(!cfg.active);
    assert_eq!(client.get_alert_active(&id), Some(false));
}

#[test]
fn test_deactivate_alert_by_admin_unauthorized() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);

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

    assert!(client.get_alert(&id).unwrap().active);
}

#[test]
fn test_deactivate_alert_by_admin_not_found() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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
    assert!(!env.events().all().is_empty());
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let inputs = vec![&env, alert_input(&env, &owner, &target, "A0")];

    let ids = client.batch_register_alert(&inputs);
    assert_eq!(ids.len(), 1);
    assert!(client.get_alert(&ids.get(0).unwrap()).is_some());
    assert_eq!(client.get_alert_count(), 1);
}

#[test]
fn test_batch_register_alert_five() {
    let (env, client) = setup();
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut inputs: Vec<AlertInput> = vec![&env];
    for _ in 0..25u32 {
        inputs.push_back(alert_input(&env, &owner, &target, "A"));
    }

    let ids = client.batch_register_alert(&inputs);
    assert_eq!(ids.len(), 25);
    assert_eq!(client.get_alert_count(), 25);
    assert_eq!(client.get_active_alert_count(&owner), 25);
}

#[test]
fn test_batch_register_alert_rolls_back_on_validation_error() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // A second owner: a repeated require_auth for the same address within one
    // invocation aborts before validation runs, which would mask the error.
    let other_owner = Address::generate(&env);
    let mut bad = alert_input(&env, &other_owner, &target, "Bad");
    // 129 bytes: one over the label limit.
    bad.label = str(&env, &"a".repeat(129));

    let inputs = vec![
        &env,
        alert_input(&env, &owner, &target, "Good"),
        bad,
    ];

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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env],
    );

    client.batch_remove_alert(&owner, &vec![&env, id]);
    assert!(client.get_alert(&id).is_none());
}

#[test]
fn test_batch_remove_alert_five() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut ids: Vec<u64> = vec![&env];
    for _ in 0..5u32 {
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "A"),
            &hash64(&env),
            &vec![&env],
        );
        ids.push_back(id);
    }

    client.batch_remove_alert(&owner, &ids);
    for i in 0..ids.len() {
        assert!(client.get_alert(&ids.get(i).unwrap()).is_none());
    }
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

#[test]
fn test_batch_remove_alert_boundary_size() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut ids: Vec<u64> = vec![&env];
    for _ in 0..25u32 {
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "A"),
            &hash64(&env),
            &vec![&env],
        );
        ids.push_back(id);
    }

    client.batch_remove_alert(&owner, &ids);
    assert_eq!(client.get_active_alert_count(&owner), 0);
}

#[test]
fn test_batch_remove_alert_unauthorized_rolls_back() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env],
    );
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
    assert!(client.get_alert(&id1).is_some());
    assert!(client.get_alert(&id2).is_some());
}

#[test]
fn test_batch_remove_alert_not_found() {
    let (env, client) = setup();
    let owner = Address::generate(&env);

    assert_eq!(
        client
            .try_batch_remove_alert(&owner, &vec![&env, 999u64])
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

#[test]
fn test_batch_remove_alert_ignores_duplicate_ids() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.batch_remove_alert(&owner, &vec![&env, id, id]);

    assert!(client.get_alert(&owner, &id).unwrap().is_none());
}

// ── Consolidated tests from lib.rs ──────────────────────────────────────

fn setup_with_watcher_registry() -> (
    Env,
    AlertRegistryClient<'static>,
    watcher_registry::WatcherRegistryClient<'static>,
) {
    use watcher_registry::WatcherRegistry;
    let env = Env::default();
    env.mock_all_auths();

    let alert_id = env.register(AlertRegistry, ());
    let watcher_id = env.register(WatcherRegistry, ());

    let alert_client = AlertRegistryClient::new(&env, &alert_id);
    let watcher_client = watcher_registry::WatcherRegistryClient::new(&env, &watcher_id);

    (env, alert_client, watcher_client)
}

#[test]
fn test_global_alert_limit_defaults_to_zero_unlimited() {
    let (_env, client) = setup();
    assert_eq!(client.get_global_alert_limit(), 0u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_global_alert_limit_enforced_across_owners() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
fn test_global_alert_limit_is_released_by_removal() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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

    let replacement = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );
    assert_eq!(replacement, 1);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_global_alert_limit_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.set_auths(&[]);
    client.set_global_alert_limit(&admin, &5u32);
}

#[test]
fn test_set_global_alert_limit_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);

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
    let (_env, client) = setup();
    assert_eq!(client.get_per_contract_alert_limit(), 0u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_per_contract_alert_limit_enforced_across_owners() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.set_auths(&[]);
    client.set_per_contract_alert_limit(&admin, &5u32);
}

#[test]
fn test_set_per_contract_alert_limit_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);

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
    use soroban_sdk::{symbol_short, testutils::Events as _};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_per_contract_alert_limit(&admin, &7u32);

    let events = env.events().all();
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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Start at 0
    assert_eq!(client.get_alert_count(), 0);

    // Cycle 1: register -> count goes to 1
    let id1 =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 1);
    // remove -> count stays at 1 (monotonic)
    client.remove_alert(&owner, &id1);
    assert_eq!(client.get_alert_count(), 1);

    // Cycle 2: register -> count goes to 2
    let id2 =
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);
    assert_eq!(client.get_alert_count(), 2);
    // remove -> count stays at 2
    client.remove_alert(&owner, &id2);
    assert_eq!(client.get_alert_count(), 2);

    // Cycle 3: register -> count goes to 3
    let id3 =
        client.register_alert(&owner, &target, &str(&env, "C"), &hash64(&env), &vec![&env]);
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
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let querier = Address::generate(&env);
    assert_eq!(client.get_alerts_by_owner(&querier, &owner).len(), 0);
}

// 8b. get_alert_ids_by_owner — thin ID-only wrapper over the owner index (#35)
#[test]
fn test_get_alert_ids_by_owner() {
    let (env, client) = setup();
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

    let ids = client.get_alert_ids_by_owner(&owner);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);

    // Unrelated owner still sees an empty list.
    assert_eq!(client.get_alert_ids_by_owner(&other).len(), 0);
}

// 10. Paginated queries work without watcher gating
#[test]
fn test_paginated_queries_no_gating() {
    let (env, client) = setup();
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
    let (env, client) = setup();
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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);
    watcher_client.register_watcher(&admin, &watcher);

    // Point alert registry at the watcher registry
    alert_client.initialize(&admin);
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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);

    alert_client.initialize(&admin);
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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);
    watcher_client.register_watcher(&admin, &watcher);

    alert_client.initialize(&admin);
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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);
    watcher_client.register_watcher(&admin, &watcher);

    alert_client.initialize(&admin);
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
    let (_env, client) = setup();
    assert!(client.get_watcher_registry().is_none());
    assert!(!client.is_watcher_gating_enabled());
}

// 16. set_watcher_registry persists and get_watcher_registry returns it
#[test]
#[cfg(feature = "testutils")]
fn test_set_and_get_watcher_registry() {
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    alert_client.initialize(&admin);

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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();
    assert!(!alert_client.is_watcher_gating_enabled());

    let admin = Address::generate(&env);
    alert_client.initialize(&admin);

    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);

    assert!(alert_client.is_watcher_gating_enabled());
}

// 17. Only admin can set watcher registry
#[test]
fn test_set_watcher_registry_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let fake_registry = Address::generate(&env);

    client.initialize(&admin);

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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // A real, deployed contract — but not a WatcherRegistry, so it has
    // no `is_watcher_authorized` entry point for the probe to find.
    let not_a_watcher_registry = env.register(AlertRegistry, ());

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
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

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
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();
    let admin = Address::generate(&env);
    alert_client.initialize(&admin);

    let bogus = env.register(AlertRegistry, ());
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
// 17b. clear_watcher_registry disables gating; set_watcher_registry can
// re-enable it afterward.
#[test]
#[cfg(feature = "testutils")]
fn test_clear_watcher_registry_disables_then_reconfigure() {
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);
    watcher_client.register_watcher(&admin, &watcher);

    alert_client.initialize(&admin);
    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());

    alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Gating active: unregistered querier is rejected.
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );

    // Clear gating.
    alert_client.clear_watcher_registry(&admin);
    assert!(alert_client.get_watcher_registry().is_none());
    assert!(!alert_client.is_watcher_gating_enabled());

    // Any querier can now read.
    assert_eq!(
        alert_client
            .get_alerts_for_contract(&stranger, &target)
            .len(),
        1
    );

    // Re-configure gating.
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
    assert_eq!(
        alert_client
            .get_alerts_for_contract(&watcher, &target)
            .len(),
        1
    );
}

// 17c. clear_watcher_registry rejects non-admin callers.
#[test]
fn test_clear_watcher_registry_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(
        client
            .try_clear_watcher_registry(&attacker)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 17d. clear_watcher_registry requires the contract to be initialized.
#[test]
fn test_clear_watcher_registry_not_initialized() {
    let (env, client) = setup();
    let admin = Address::generate(&env);

    assert_eq!(
        client
            .try_clear_watcher_registry(&admin)
            .unwrap_err()
            .unwrap(),
        ContractError::NotInitialized
    );
}

// 18. updated_at is strictly greater than created_at after update_alert
//
// The Soroban test environment starts with timestamp 0 and does not
// advance automatically. We manually bump the ledger timestamp by 1
// second between registration and update so that the contract's
// `env.ledger().timestamp()` call inside `update_alert` returns a
// value that is strictly greater than the one captured at registration.
#[test]
fn test_updated_at_strictly_greater_than_created_at() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register at timestamp T (default = 0).
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Timestamp Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let before = client.get_alert(&owner, &id).unwrap();
    assert_eq!(
        before.created_at, before.updated_at,
        "created_at and updated_at should be equal right after registration"
    );

    // Advance the ledger clock by 1 second so the update lands at T+1.
    env.ledger().with_mut(|li| {
        li.timestamp += 1;
    });

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true);

    let after = client.get_alert(&owner, &id).unwrap();
    assert!(
        after.updated_at > after.created_at,
        "updated_at ({}) must be strictly greater than created_at ({})",
        after.updated_at,
        after.created_at
    );
}

// 19. Register an alert with exactly 50 valid rule strings.
//
// This verifies that the contract handles the maximum allowed rule count
// without hitting Soroban instruction limits. We alternate between the
// two valid rule descriptors ("rule:transfer" and "rule:mint") to fill
// all 50 slots, then confirm every entry is stored correctly.
#[test]
fn test_register_alert_with_50_rules_no_instruction_limit() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Build a vec of 50 valid rules, alternating between the two
    // accepted descriptors so the list is realistic.
    let mut rules: Vec<String> = vec![&env];
    for i in 0..50u32 {
        let rule = if i % 2 == 0 {
            str(&env, "rule:transfer")
        } else {
            str(&env, "rule:mint")
        };
        rules.push_back(rule);
    }

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Bulk Rules Alert"),
        &hash64(&env),
        &rules,
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.rules.len(), 50, "all 50 rules should be persisted");

    // Spot-check a few entries to confirm data integrity.
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.rules.get(1).unwrap(), str(&env, "rule:mint"));
    assert_eq!(cfg.rules.get(48).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.rules.get(49).unwrap(), str(&env, "rule:mint"));
}

// ── Feature A: update_label ───────────────────────────────────────────────

// 18. Happy path — update_label changes only the label
#[test]
fn test_update_label_changes_label() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Original"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(
        client
            .try_update_label(&owner, &id, &str(&env, "Renamed"))
            .unwrap(),
        Ok(())
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.label, str(&env, "Renamed"));
    // rules and webhook_hash must be untouched
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.webhook_hash, hash64(&env));
    assert!(cfg.active);
}

// 19. update_label — unauthorized caller is rejected
#[test]
fn test_update_label_unauthorized() {
    let (env, client) = setup();
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
            .try_update_label(&attacker, &id, &str(&env, "Hacked"))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 20. update_label — nonexistent alert returns AlertNotFound
#[test]
fn test_update_label_not_found() {
    let (env, client) = setup();
    let caller = Address::generate(&env);

    assert_eq!(
        client
            .try_update_label(&caller, &999u64, &str(&env, "X"))
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 21. update_label — label exceeding 128 bytes is rejected
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_label_too_long() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_label(&owner, &id, &str(&env, &"a".repeat(129)));
}

// 22. update_label — exactly 128 bytes is accepted
#[test]
fn test_update_label_max_length_accepted() {
    let (env, client) = setup();
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
            .try_update_label(&owner, &id, &str(&env, &"a".repeat(128)))
            .unwrap(),
        Ok(())
    );
}

// ── Feature B: get_active_alerts_for_contract ─────────────────────────────

// 23. Happy path — only active alerts are returned
#[test]
fn test_get_active_alerts_for_contract_filters_inactive() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Active"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Inactive"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    // Deactivate the second alert
    client.update_alert(&owner, &id2, &vec![&env, str(&env, "rule:mint")], &false);

    let all = client.get_alerts_for_contract(&owner, &target);
    assert_eq!(all.len(), 2);

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 1);
    assert_eq!(active.get(0).unwrap().label, str(&env, "Active"));
    let _ = id1;
}

// 24. get_active_alerts_for_contract — returns empty when all are inactive
#[test]
fn test_get_active_alerts_for_contract_all_inactive() {
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

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &false);

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 0);
}

// 25. get_active_alerts_for_contract — returns empty for unknown contract
#[test]
fn test_get_active_alerts_for_contract_empty() {
    let (env, client) = setup();
    let target = Address::generate(&env);
    assert_eq!(
        client
            .get_active_alerts_for_contract(&Address::generate(&env), &target)
            .len(),
        0
    );
}

// 26. get_active_alerts_for_contract — all active alerts are returned
#[test]
fn test_get_active_alerts_for_contract_all_active() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 2);
}

// 18. transfer_admin emits an ("admin", "transfer") event
#[test]
fn test_transfer_admin_emits_event() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    // Verify at least one event was published during the transfer
    assert!(!env.events().all().is_empty());
}

// 19. old admin cannot act after transfer_admin
#[test]
fn test_old_admin_rejected_for_remove_alert_by_admin() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let new_admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // old admin can no longer perform admin actions
    assert_eq!(
        client
            .try_remove_alert_by_admin(&admin, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// ── get_alerts_modified_since ─────────────────────────────────────────────

// 18. Returns all alerts when since == 0
#[test]
fn test_get_alerts_modified_since_zero_returns_all() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 2);
}

// 19. Returns empty vec when no alerts exist
#[test]
fn test_get_alerts_modified_since_empty_registry() {
    let (_env, client) = setup();
    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 0);
}

// 20. Filters out alerts whose updated_at is before `since`
#[test]
fn test_get_alerts_modified_since_filters_old_alerts() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register at ledger timestamp 0 (default in tests)
    let _id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Old"),
        &hash64(&env),
        &vec![&env],
    );

    // Advance the ledger timestamp so the next alert has a higher updated_at
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let _id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "New"),
        &hash64(&env),
        &vec![&env],
    );

    // Query with since = 1000 — should only return the second alert
    let results = client.get_alerts_modified_since(&1000u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "New"));
}

// 21. An updated alert appears in a subsequent incremental sync
#[test]
fn test_get_alerts_modified_since_includes_updated_alert() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register both alerts at timestamp 0
    let id1 =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    let _id2 =
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    // Advance time and update the first alert
    env.ledger().with_mut(|li| li.timestamp = 500);
    client.update_alert(&owner, &id1, &vec![&env], &false);

    // Incremental sync from timestamp 500 should return only the updated alert
    let results = client.get_alerts_modified_since(&500u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "A"));
}

// 22. Removed alerts are not returned
#[test]
fn test_get_alerts_modified_since_excludes_removed_alerts() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    client.remove_alert(&owner, &id1);

    // Only the surviving alert should be returned
    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "B"));
}

// 23. since is exclusive of nothing — boundary value exactly equal is included
#[test]
fn test_get_alerts_modified_since_boundary_inclusive() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 42);
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Boundary"),
        &hash64(&env),
        &vec![&env],
    );

    // since == updated_at should be inclusive
    let results = client.get_alerts_modified_since(&42u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);

    // since == updated_at + 1 should exclude it
    let results_after = client.get_alerts_modified_since(&43u64, &0u32, &u32::MAX);
    assert_eq!(results_after.len(), 0);
}

// ── Auth-failure tests (no mock_all_auths) ────────────────────────────────

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_register_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    // register with mocked auth first, then call update without auth
    env.mock_all_auths();
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    env.set_auths(&[]);
    client.update_alert(&owner, &id, &vec![&env], &false);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_webhook(&owner, &id, &hash64c(&env, 'b'));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_remove_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.remove_alert(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_transfer_admin_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let new_admin = Address::generate(&env);
    env.set_auths(&[]);
    client.transfer_admin(&admin, &new_admin);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_per_owner_alert_limit_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.set_auths(&[]);
    client.set_per_owner_alert_limit(&admin, &5u32);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_remove_alert_by_admin_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    env.set_auths(&[]);
    client.remove_alert_by_admin(&admin, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_watcher_registry_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let watcher_registry = Address::generate(&env);
    env.set_auths(&[]);
    client.set_watcher_registry(&admin, &watcher_registry);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_propose_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_confirm_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
    env.set_auths(&[]);
    client.confirm_webhook(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_renew_alert_ttl_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.renew_alert_ttl(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_label_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_label(&owner, &id, &str(&env, "New Label"));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_deactivate_all_alerts_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.deactivate_all_alerts(&owner);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_target_contract_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_target_contract(&owner, &id, &new_target);
}

// ── Load & Scan-Cost Benchmarks (Issues #38, #39, #116) ───────────────────

/// Load test quantifying the O(N) full-scan cost of `get_alerts_modified_since` (#38).
/// Registers N alerts and benchmarks the CPU instruction cost of scanning the registry,
/// establishing an upper bound budget regression guard.
#[test]
fn test_load_get_alerts_modified_since_instruction_cost() {
    const N: usize = 50;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    for i in 0..N {
        let label = str(&env, "Alert");
        client.register_alert(&owner, &target, &label, &hash, &rules);
        // Stagger timestamps so every alert has a distinct updated_at
        if i % 5 == 0 {
            env.ledger().with_mut(|li| li.timestamp += 1);
        }
    }

    // Measure scan instruction cost across all N alerts
    let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
    let modified = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let scan_cost = cpu_after.saturating_sub(cpu_before);

    assert_eq!(modified.len() as usize, N);
    // Assert an upper bound regression guard on the scan cost for N=50
    assert!(
        scan_cost < 15_000_000,
        "get_alerts_modified_since cost {scan_cost} exceeded upper bound 15M instructions"
    );
}

/// Confirms the fix for #38: a caller that pages with a small, bounded
/// `limit` pays a scan cost proportional to that `limit`, not to the
/// total number of alerts ever registered. This is what stops an
/// attacker from inflating `NEXT_ID` (via repeated `register_alert`
/// calls) from degrading the read path for every other caller — each
/// caller controls their own scan cost via `limit`.
#[test]
fn test_get_alerts_modified_since_pagination_bounds_scan_cost() {
    const N: u32 = 400;
    const PAGE: u32 = 10;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    for _ in 0..N {
        let label = str(&env, "Alert");
        client.register_alert(&owner, &target, &label, &hash, &rules);
    }
    assert_eq!(client.get_alert_count(), u64::from(N));

    // A small page near the end of a large registry must still be cheap:
    // cost is bounded by PAGE, not by N.
    let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
    let page = client.get_alerts_modified_since(&0u64, &390u32, &PAGE);
    let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let page_scan_cost = cpu_after.saturating_sub(cpu_before);

    assert_eq!(page.len() as usize, PAGE as usize);
    // A page of 10 out of a 400-alert registry should cost nowhere near
    // the ~15M-instruction ceiling asserted for a full 50-alert scan
    // above — if this ever regresses to an O(N) scan the cost will blow
    // well past this bound.
    assert!(
        page_scan_cost < 2_000_000,
        "paginated get_alerts_modified_since cost {page_scan_cost} exceeded upper bound 2M instructions for a page of {PAGE}"
    );

    // Requesting past the end of the registry returns an empty page
    // rather than scanning anything.
    let empty_page = client.get_alerts_modified_since(&0u64, &N, &PAGE);
    assert_eq!(empty_page.len(), 0);
}

/// Load test quantifying the repeated rescan cost in `assert_per_owner_limit` (#39).
/// Registers alerts with an active per-owner limit and benchmarks instruction growth,
/// asserting an upper bound regression guard.
/// Load test quantifying the (formerly O(n²)) cost of `assert_per_owner_limit` (#39).
///
/// Registers `LIMIT` alerts for the same owner under an active per-owner
/// limit and benchmarks instruction growth across the run. Before the
/// fix, `assert_per_owner_limit` rescanned `get_active_alert_count` (an
/// O(n) full-index scan) on every call, so `last_reg_cost` grew roughly
/// linearly with `LIMIT` — at LIMIT=100 the last call cost ~100x the
/// first. With the running per-owner counter, the limit check is O(1),
/// so cost per registration should stay flat regardless of `LIMIT`.
#[test]
fn test_load_assert_per_owner_limit_instruction_cost() {
    const LIMIT: u32 = 100;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_per_owner_alert_limit(&admin, &LIMIT);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env];

    let mut first_reg_cost: u64 = 0;
    let mut last_reg_cost: u64 = 0;

    let total_cpu_before = env.cost_estimate().budget().cpu_instruction_cost();

    for i in 0..LIMIT {
        let label = str(&env, "LimitLoadAlert");
        let before = env.cost_estimate().budget().cpu_instruction_cost();
        client.register_alert(&owner, &target, &label, &hash, &rules);
        let after = env.cost_estimate().budget().cpu_instruction_cost();
        let cost = after.saturating_sub(before);

        if i == 0 {
            first_reg_cost = cost;
        } else if i == LIMIT - 1 {
            last_reg_cost = cost;
        }
    }

    let total_cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let total_registration_cost = total_cpu_after.saturating_sub(total_cpu_before);

    // Quantify that cost per registration includes the owner scan overhead
    assert!(
        first_reg_cost > 0 && last_reg_cost > 0,
        "Registration costs must be non-zero"
    );
    // Before/after regression guard: with an O(1) per-owner counter, the
    // Nth registration should not cost meaningfully more than the 1st.
    // (Under the old O(n) rescan, this ratio grew with LIMIT itself.)
    assert!(
        last_reg_cost < first_reg_cost.saturating_mul(3),
        "registration cost grew from {first_reg_cost} to {last_reg_cost} across {LIMIT} \
         calls — assert_per_owner_limit is no longer O(1)"
    );
    // Assert an upper bound regression guard on total batch registration cost with limit checks
    assert!(
        total_registration_cost < 50_000_000,
        "Total registration cost {total_registration_cost} exceeded upper bound 50M instructions"
    );
}

/// `get_non_removed_alert_count` is O(1) regardless of how many alerts an
/// owner has ever registered — it reads a maintained counter instead of
/// rescanning `OwnerIndex` (#39).
#[test]
fn test_get_non_removed_alert_count_instruction_cost_is_constant() {
    const N: u32 = 200;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env];

    for _ in 0..N {
        client.register_alert(&owner, &target, &str(&env, "Alert"), &hash, &rules);
    }

    let before = env.cost_estimate().budget().cpu_instruction_cost();
    let count = client.get_non_removed_alert_count(&owner);
    let after = env.cost_estimate().budget().cpu_instruction_cost();
    let cost = after.saturating_sub(before);

    assert_eq!(count, N);
    // An O(n) rescan at N=200 would cost far more than a single storage
    // read; this bound would fail under a scan-based implementation.
    assert!(
        cost < 200_000,
        "get_non_removed_alert_count cost {cost} at N={N} looks O(n), not O(1)"
    );
}

// #63 — ID monotonicity: each successive register_alert returns prev+1
#[test]
fn test_id_monotonicity() {
    const N: u64 = 10;

    let (env, client) = setup();

    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut prev_id: Option<u64> = None;
    for i in 0..N {
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "alert"),
            &hash64(&env),
            &vec![&env],
        );
        if let Some(p) = prev_id {
            assert_eq!(
                id,
                p + 1,
                "expected id {} but got {} at iteration {}",
                p + 1,
                id,
                i
            );
        }
        prev_id = Some(id);
    }
}

// #33 — update_target_contract moves the alert to a new contract index
#[test]
fn test_update_target_contract() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let old_target = Address::generate(&env);
    let new_target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &old_target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_target_contract(&owner, &id, &new_target);

    // alert config reflects new target
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.target_contract, new_target);

    // indexes updated correctly
    assert_eq!(client.get_alerts_for_contract(&owner, &old_target).len(), 0);
    assert_eq!(client.get_alerts_for_contract(&owner, &new_target).len(), 1);
}

// #33 — update_target_contract unauthorized
#[test]
fn test_update_target_contract_unauthorized() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_update_target_contract(&attacker, &id, &new_target)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 18. update_alert after remove_alert returns AlertNotFound
#[test]
fn test_update_alert_after_remove_returns_not_found() {
    let (env, client) = setup();
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

    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env], &false)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 19. update_webhook after remove_alert returns AlertNotFound
#[test]
fn test_update_webhook_after_remove_returns_not_found() {
    let (env, client) = setup();
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

    assert_eq!(
        client
            .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 18. get_alert_active returns None for nonexistent ID
#[test]
fn test_get_alert_active_nonexistent() {
    let (env, client) = setup();
    assert!(client
        .get_alert_active(&Address::generate(&env), &999u64)
        .is_none());
}

// 19. get_alert_active returns true after registration
#[test]
fn test_get_alert_active_after_register() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
}

// 20. get_alert_active reflects update_alert changes
#[test]
fn test_get_alert_active_after_update() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));

    client.update_alert(&owner, &id, &vec![&env], &false);
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));

    client.update_alert(&owner, &id, &vec![&env], &true);
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
}

// 21. get_alert_active returns None after removal
#[test]
fn test_get_alert_active_after_remove() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
    client.remove_alert(&owner, &id);
    assert!(client.get_alert_active(&owner, &id).is_none());
}

// 21a. get_alert_owner returns the owner after registration
#[test]
fn test_get_alert_owner_after_register() {
    let (env, client) = setup();
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
        client.get_alert_owner(&owner, &id).unwrap(),
        Some(owner)
    );
}

// 21b. get_alert_owner reflects an accepted ownership transfer
#[test]
fn test_get_alert_owner_after_transfer() {
    let (env, client) = setup();
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

    client.propose_alert_transfer(&owner, &id, &new_owner);

    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(
        client.get_alert_owner(&owner, &id).unwrap(),
        Some(new_owner)
    );
}

// 21c. get_alert_owner returns None for nonexistent ID
#[test]
fn test_get_alert_owner_nonexistent() {
    let (env, client) = setup();
    assert!(client
        .get_alert_owner(&Address::generate(&env), &999u64)
        .is_none());
}

// 22. deactivate_all_alerts returns 0 when owner has no alerts
#[test]
fn test_deactivate_all_alerts_empty() {
    let (env, client) = setup();
    let owner = Address::generate(&env);

    assert_eq!(client.deactivate_all_alerts(&owner), 0);
}

// 23. deactivate_all_alerts deactivates all alerts for the owner
#[test]
fn test_deactivate_all_alerts_multiple() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );
    let id3 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 3"),
        &hash64c(&env, '3'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(client.get_alert_active(&owner, &id1), Some(true));
    assert_eq!(client.get_alert_active(&owner, &id2), Some(true));
    assert_eq!(client.get_alert_active(&owner, &id3), Some(true));

    let count = client.deactivate_all_alerts(&owner);
    assert_eq!(count, 3);

    assert_eq!(client.get_alert_active(&owner, &id1), Some(false));
    assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
    assert_eq!(client.get_alert_active(&owner, &id3), Some(false));
}

// 24. deactivate_all_alerts only affects the calling owner's alerts
#[test]
fn test_deactivate_all_alerts_other_owner_unaffected() {
    let (env, client) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner1,
        &target,
        &str(&env, "Owner1 Alert"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner2,
        &target,
        &str(&env, "Owner2 Alert"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    let count = client.deactivate_all_alerts(&owner1);
    assert_eq!(count, 1);

    assert_eq!(client.get_alert_active(&Address::generate(&env), &id1), Some(false));
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id2), Some(true));
}

// 25. deactivate_all_alerts skips removed alerts and deactivates remaining
#[test]
fn test_deactivate_all_alerts_after_removal() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    client.remove_alert(&owner, &id1);

    let count = client.deactivate_all_alerts(&owner);
    assert_eq!(count, 1);

    // id1 is gone
    assert!(client.get_alert(&owner, &id1).is_none());
    // id2 is now inactive
    assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
}

// 18. get_alerts_by_owner_paginated — basic pagination
#[test]
fn test_get_alerts_by_owner_paginated() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for label in ["A", "B", "C", "D", "E"] {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env],
        );
    }

    // first page
    let page1 = client.get_alerts_by_owner_paginated(&owner, &owner, &0u32, &3u32);
    assert_eq!(page1.len(), 3);

    // second page
    let page2 = client.get_alerts_by_owner_paginated(&owner, &owner, &3u32, &3u32);
    assert_eq!(page2.len(), 2);

    // offset beyond length returns empty
    let empty = client.get_alerts_by_owner_paginated(&owner, &owner, &10u32, &3u32);
    assert_eq!(empty.len(), 0);
}

// 19. get_contract_alerts_paginated — basic pagination
#[test]
fn test_get_contract_alerts_paginated() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for label in ["A", "B", "C", "D"] {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env],
        );
    }

    let page = client.get_contract_alerts_paginated(&owner, &target, &1u32, &2u32);
    assert_eq!(page.len(), 2);
}

// 18. get_admin panics with NotInitialized when contract is not initialized
// (Result-returning contract functions still panic via the plain client
// call when they return Err — this mirrors WatcherRegistry::get_admin.)
#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_admin_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    client.get_admin();
}

// 18b. get_admin returns a typed NotInitialized error via try_get_admin (#41)
#[test]
fn test_try_get_admin_uninitialized() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);

    assert_eq!(
        client.try_get_admin().unwrap_err().unwrap(),
        ContractError::NotInitialized
    );
}

// 18c. get_admin returns Ok(admin) once initialized
#[test]
fn test_try_get_admin_after_initialize() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(client.try_get_admin().unwrap().unwrap(), admin);
    assert_eq!(client.get_admin(), admin);
}

// 19. Alert can be deactivated and reactivated via update_alert
#[test]
fn test_alert_deactivate_reactivate() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:mint")],
    );

    // deactivate
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false)
            .unwrap(),
        Ok(())
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(!cfg.active);

    // reactivate
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true)
            .unwrap(),
        Ok(())
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(cfg.active);
}

// 18. update_webhook advances updated_at beyond its value at registration
#[test]
fn test_update_webhook_updates_timestamp() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);

    let original_updated_at = client.get_alert(&owner, &id).unwrap().updated_at;
    env.ledger().set_timestamp(original_updated_at + 100);

    client
        .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
        .unwrap()
        .unwrap();

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(cfg.updated_at > original_updated_at);
}

// 19. Multiple owners watching the same contract — indexes are isolated per owner
#[test]
fn test_multiple_owners_overlapping_target_contract() {
    let (env, client) = setup();
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner_a,
        &target,
        &str(&env, "Alert-A"),
        &hash64c(&env, '5'),
        &vec![&env],
    );
    client.register_alert(
        &owner_b,
        &target,
        &str(&env, "Alert-B"),
        &hash64c(&env, '6'),
        &vec![&env],
    );

    assert_eq!(client.get_alerts_for_contract(&owner_a, &target).len(), 2);

    let alerts_a = client.get_alerts_by_owner(&owner_a, &owner_a);
    assert_eq!(alerts_a.len(), 1);
    assert_eq!(alerts_a.get(0).unwrap().owner, owner_a);

    let alerts_b = client.get_alerts_by_owner(&owner_b, &owner_b);
    assert_eq!(alerts_b.len(), 1);
    assert_eq!(alerts_b.get(0).unwrap().owner, owner_b);
}

// ── Feature B: configurable TTL via bump_alert ────────────────────────────

// B-1. bump_alert succeeds for an existing alert
#[test]
fn test_bump_alert_succeeds() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.try_bump_alert(&id, &17_280u32).unwrap(), Ok(()));
}

// B-2. bump_alert returns AlertNotFound for a non-existent ID
#[test]
fn test_bump_alert_not_found() {
    let (_env, client) = setup();
    assert_eq!(
        client
            .try_bump_alert(&999u64, &17_280u32)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// B-3. bump_alert clamps TTL above MAX_TTL to MAX_TTL
#[test]
fn test_bump_alert_clamps_to_max_ttl() {
    use soroban_sdk::testutils::Events as _;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Request a TTL above the protocol maximum
    client.bump_alert(&id, &u32::MAX);

    // The emitted event should carry the clamped effective TTL
    let events = env.events().all();
    let bump_event = events.iter().find(|(_, topics, _)| {
        topics.len() == 2
            && Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("alert")
            && Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("bump")
    });
    assert!(bump_event.is_some(), "expected an alert.bump event");

    let (_, _, data) = bump_event.unwrap();
    let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_id, id);
    assert_eq!(emitted_ttl, MAX_TTL, "TTL must be clamped to MAX_TTL");
}

// B-4. bump_alert with TTL below MAX_TTL uses the requested value exactly
#[test]
fn test_bump_alert_uses_requested_ttl_when_below_max() {
    use soroban_sdk::testutils::Events as _;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    let requested_ttl: u32 = 120_960; // ~7 days, well below MAX_TTL
    client.bump_alert(&id, &requested_ttl);

    let events = env.events().all();
    let bump_event = events.iter().find(|(_, topics, _)| {
        topics.len() == 2
            && Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("alert")
            && Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("bump")
    });
    assert!(bump_event.is_some());

    let (_, _, data) = bump_event.unwrap();
    let (_, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_ttl, requested_ttl);
}

// B-5. bump_alert emits the correct event shape (topic + data)
#[test]
fn test_bump_alert_event_shape() {
    use soroban_sdk::{symbol_short, testutils::Events as _};

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    let ttl: u32 = 17_280;
    client.bump_alert(&id, &ttl);

    let events = env.events().all();
    let bump_event = events
        .iter()
        .find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("alert")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("bump")
        })
        .expect("alert.bump event must be emitted");

    // Verify data shape: (id: u64, ttl: u32)
    let (_, _, data) = bump_event;
    let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_id, id);
    assert_eq!(emitted_ttl, ttl);
}

// B-6. bump_alert does not modify the alert's content
#[test]
fn test_bump_alert_does_not_modify_content() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Immutable"),
        &hash64c(&env, 'c'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let before = client.get_alert(&owner, &id).unwrap();
    client.bump_alert(&id, &17_280u32);
    let after = client.get_alert(&owner, &id).unwrap();

    // All fields must be identical after a bump
    assert_eq!(after.label, before.label);
    assert_eq!(after.webhook_hash, before.webhook_hash);
    assert_eq!(after.rules.len(), before.rules.len());
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.target_contract, before.target_contract);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.active, before.active);
}

// B-7. DEFAULT_TTL and MAX_TTL constants have the expected values
#[test]
fn test_ttl_constants() {
    // MAX_TTL must be strictly greater than DEFAULT_TTL (checked at compile time)
    const _: () = assert!(MAX_TTL > DEFAULT_TTL);

    // DEFAULT_TTL ≈ 24 hours at 5 s/ledger
    assert_eq!(DEFAULT_TTL, 17_280);
    // MAX_TTL ≈ 31 days at 5 s/ledger
    assert_eq!(MAX_TTL, 535_680);
}
}

// 17b. clear_watcher_registry disables gating; set_watcher_registry can
// re-enable it afterward.
#[test]
#[cfg(feature = "testutils")]
fn test_clear_watcher_registry_disables_then_reconfigure() {
    let (env, alert_client, watcher_client) = setup_with_watcher_registry();

    let admin = Address::generate(&env);
    let watcher = Address::generate(&env);
    let stranger = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    watcher_client.initialize(&admin);
    watcher_client.register_watcher(&admin, &watcher);

    alert_client.initialize(&admin);
    let watcher_contract_id = watcher_client.address.clone();
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());

    alert_client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Gating active: unregistered querier is rejected.
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );

    // Clear gating.
    alert_client.clear_watcher_registry(&admin);
    assert!(alert_client.get_watcher_registry().is_none());
    assert!(!alert_client.is_watcher_gating_enabled());

    // Any querier can now read.
    assert_eq!(
        alert_client
            .get_alerts_for_contract(&stranger, &target)
            .len(),
        1
    );

    // Re-configure gating.
    alert_client.set_watcher_registry(&admin, &watcher_contract_id);
    assert!(alert_client.is_watcher_gating_enabled());
    assert_eq!(
        alert_client
            .try_get_alerts_for_contract(&stranger, &target)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAWatcher
    );
    assert_eq!(
        alert_client
            .get_alerts_for_contract(&watcher, &target)
            .len(),
        1
    );
}

// 17c. clear_watcher_registry rejects non-admin callers.
#[test]
fn test_clear_watcher_registry_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(
        client
            .try_clear_watcher_registry(&attacker)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 17d. clear_watcher_registry requires the contract to be initialized.
#[test]
fn test_clear_watcher_registry_not_initialized() {
    let (env, client) = setup();
    let admin = Address::generate(&env);

    assert_eq!(
        client
            .try_clear_watcher_registry(&admin)
            .unwrap_err()
            .unwrap(),
        ContractError::NotInitialized
    );
}

// 18. updated_at is strictly greater than created_at after update_alert
//
// The Soroban test environment starts with timestamp 0 and does not
// advance automatically. We manually bump the ledger timestamp by 1
// second between registration and update so that the contract's
// `env.ledger().timestamp()` call inside `update_alert` returns a
// value that is strictly greater than the one captured at registration.
#[test]
fn test_updated_at_strictly_greater_than_created_at() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register at timestamp T (default = 0).
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Timestamp Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let before = client.get_alert(&owner, &id).unwrap();
    assert_eq!(
        before.created_at, before.updated_at,
        "created_at and updated_at should be equal right after registration"
    );

    // Advance the ledger clock by 1 second so the update lands at T+1.
    env.ledger().with_mut(|li| {
        li.timestamp += 1;
    });

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true);

    let after = client.get_alert(&owner, &id).unwrap();
    assert!(
        after.updated_at > after.created_at,
        "updated_at ({}) must be strictly greater than created_at ({})",
        after.updated_at,
        after.created_at
    );
}

// 19. Register an alert with exactly 50 valid rule strings.
//
// This verifies that the contract handles the maximum allowed rule count
// without hitting Soroban instruction limits. We alternate between the
// two valid rule descriptors ("rule:transfer" and "rule:mint") to fill
// all 50 slots, then confirm every entry is stored correctly.
#[test]
fn test_register_alert_with_50_rules_no_instruction_limit() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Build a vec of 50 valid rules, alternating between the two
    // accepted descriptors so the list is realistic.
    let mut rules: Vec<String> = vec![&env];
    for i in 0..50u32 {
        let rule = if i % 2 == 0 {
            str(&env, "rule:transfer")
        } else {
            str(&env, "rule:mint")
        };
        rules.push_back(rule);
    }

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Bulk Rules Alert"),
        &hash64(&env),
        &rules,
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.rules.len(), 50, "all 50 rules should be persisted");

    // Spot-check a few entries to confirm data integrity.
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.rules.get(1).unwrap(), str(&env, "rule:mint"));
    assert_eq!(cfg.rules.get(48).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.rules.get(49).unwrap(), str(&env, "rule:mint"));
}

// 18. Happy path — update_label changes only the label
#[test]
fn test_update_label_changes_label() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Original"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(
        client
            .try_update_label(&owner, &id, &str(&env, "Renamed"))
            .unwrap(),
        Ok(())
    );

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.label, str(&env, "Renamed"));
    // rules and webhook_hash must be untouched
    assert_eq!(cfg.rules.get(0).unwrap(), str(&env, "rule:transfer"));
    assert_eq!(cfg.webhook_hash, hash64(&env));
    assert!(cfg.active);
}

// 19. update_label — unauthorized caller is rejected
#[test]
fn test_update_label_unauthorized() {
    let (env, client) = setup();
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
            .try_update_label(&attacker, &id, &str(&env, "Hacked"))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 20. update_label — nonexistent alert returns AlertNotFound
#[test]
fn test_update_label_not_found() {
    let (env, client) = setup();
    let caller = Address::generate(&env);

    assert_eq!(
        client
            .try_update_label(&caller, &999u64, &str(&env, "X"))
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 21. update_label — label exceeding 128 bytes is rejected
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_label_too_long() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_label(&owner, &id, &str(&env, &"a".repeat(129)));
}

// 22. update_label — exactly 128 bytes is accepted
#[test]
fn test_update_label_max_length_accepted() {
    let (env, client) = setup();
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
            .try_update_label(&owner, &id, &str(&env, &"a".repeat(128)))
            .unwrap(),
        Ok(())
    );
}

// 23. Happy path — only active alerts are returned
#[test]
fn test_get_active_alerts_for_contract_filters_inactive() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Active"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Inactive"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    // Deactivate the second alert
    client.update_alert(&owner, &id2, &vec![&env, str(&env, "rule:mint")], &false);

    let all = client.get_alerts_for_contract(&owner, &target);
    assert_eq!(all.len(), 2);

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 1);
    assert_eq!(active.get(0).unwrap().label, str(&env, "Active"));
    let _ = id1;
}

// 24. get_active_alerts_for_contract — returns empty when all are inactive
#[test]
fn test_get_active_alerts_for_contract_all_inactive() {
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

    client.update_alert(&owner, &id, &vec![&env, str(&env, "rule:transfer")], &false);

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 0);
}

// 25. get_active_alerts_for_contract — returns empty for unknown contract
#[test]
fn test_get_active_alerts_for_contract_empty() {
    let (env, client) = setup();
    let target = Address::generate(&env);
    assert_eq!(
        client
            .get_active_alerts_for_contract(&Address::generate(&env), &target)
            .len(),
        0
    );
}

// 26. get_active_alerts_for_contract — all active alerts are returned
#[test]
fn test_get_active_alerts_for_contract_all_active() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner,
        &target,
        &str(&env, "A1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    client.register_alert(
        &owner,
        &target,
        &str(&env, "A2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    let active = client.get_active_alerts_for_contract(&owner, &target);
    assert_eq!(active.len(), 2);
}

// 18. transfer_admin emits an ("admin", "transfer") event
#[test]
fn test_transfer_admin_emits_event() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    // Verify at least one event was published during the transfer
    assert!(!env.events().all().is_empty());
}

// 19. old admin cannot act after transfer_admin
#[test]
fn test_old_admin_rejected_for_remove_alert_by_admin() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let new_admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );

    // old admin can no longer perform admin actions
    assert_eq!(
        client
            .try_remove_alert_by_admin(&admin, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 18. Returns all alerts when since == 0
#[test]
fn test_get_alerts_modified_since_zero_returns_all() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 2);
}

// 19. Returns empty vec when no alerts exist
#[test]
fn test_get_alerts_modified_since_empty_registry() {
    let (_env, client) = setup();
    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 0);
}

// 20. Filters out alerts whose updated_at is before `since`
#[test]
fn test_get_alerts_modified_since_filters_old_alerts() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register at ledger timestamp 0 (default in tests)
    let _id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Old"),
        &hash64(&env),
        &vec![&env],
    );

    // Advance the ledger timestamp so the next alert has a higher updated_at
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let _id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "New"),
        &hash64(&env),
        &vec![&env],
    );

    // Query with since = 1000 — should only return the second alert
    let results = client.get_alerts_modified_since(&1000u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "New"));
}

// 21. An updated alert appears in a subsequent incremental sync
#[test]
fn test_get_alerts_modified_since_includes_updated_alert() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Register both alerts at timestamp 0
    let id1 =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    let _id2 =
        client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    // Advance time and update the first alert
    env.ledger().with_mut(|li| li.timestamp = 500);
    client.update_alert(&owner, &id1, &vec![&env], &false);

    // Incremental sync from timestamp 500 should return only the updated alert
    let results = client.get_alerts_modified_since(&500u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "A"));
}

// 22. Removed alerts are not returned
#[test]
fn test_get_alerts_modified_since_excludes_removed_alerts() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.register_alert(&owner, &target, &str(&env, "B"), &hash64(&env), &vec![&env]);

    client.remove_alert(&owner, &id1);

    // Only the surviving alert should be returned
    let results = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap().label, str(&env, "B"));
}

// 23. since is exclusive of nothing — boundary value exactly equal is included
#[test]
fn test_get_alerts_modified_since_boundary_inclusive() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 42);
    client.register_alert(
        &owner,
        &target,
        &str(&env, "Boundary"),
        &hash64(&env),
        &vec![&env],
    );

    // since == updated_at should be inclusive
    let results = client.get_alerts_modified_since(&42u64, &0u32, &u32::MAX);
    assert_eq!(results.len(), 1);

    // since == updated_at + 1 should exclude it
    let results_after = client.get_alerts_modified_since(&43u64, &0u32, &u32::MAX);
    assert_eq!(results_after.len(), 0);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_register_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    // register with mocked auth first, then call update without auth
    env.mock_all_auths();
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    env.set_auths(&[]);
    client.update_alert(&owner, &id, &vec![&env], &false);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_webhook(&owner, &id, &hash64c(&env, 'b'));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_remove_alert_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.remove_alert(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_transfer_admin_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let new_admin = Address::generate(&env);
    env.set_auths(&[]);
    client.transfer_admin(&admin, &new_admin);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_per_owner_alert_limit_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.set_auths(&[]);
    client.set_per_owner_alert_limit(&admin, &5u32);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_remove_alert_by_admin_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "A"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:transfer")],
    );
    env.set_auths(&[]);
    client.remove_alert_by_admin(&admin, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_set_watcher_registry_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let watcher_registry = Address::generate(&env);
    env.set_auths(&[]);
    client.set_watcher_registry(&admin, &watcher_registry);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_propose_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_confirm_webhook_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    client.propose_webhook(&owner, &id, &hash64c(&env, 'p'));
    env.set_auths(&[]);
    client.confirm_webhook(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_renew_alert_ttl_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.renew_alert_ttl(&owner, &id);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_label_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_label(&owner, &id, &str(&env, "New Label"));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_deactivate_all_alerts_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    env.mock_all_auths();
    client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.deactivate_all_alerts(&owner);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_update_target_contract_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);
    env.mock_all_auths();
    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);
    env.set_auths(&[]);
    client.update_target_contract(&owner, &id, &new_target);
}

/// Load test quantifying the O(N) full-scan cost of `get_alerts_modified_since` (#38).
/// Registers N alerts and benchmarks the CPU instruction cost of scanning the registry,
/// establishing an upper bound budget regression guard.
#[test]
fn test_load_get_alerts_modified_since_instruction_cost() {
    const N: usize = 50;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    for i in 0..N {
        let label = str(&env, "Alert");
        client.register_alert(&owner, &target, &label, &hash, &rules);
        // Stagger timestamps so every alert has a distinct updated_at
        if i % 5 == 0 {
            env.ledger().with_mut(|li| li.timestamp += 1);
        }
    }

    // Measure scan instruction cost across all N alerts
    let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
    let modified = client.get_alerts_modified_since(&0u64, &0u32, &u32::MAX);
    let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let scan_cost = cpu_after.saturating_sub(cpu_before);

    assert_eq!(modified.len() as usize, N);
    // Assert an upper bound regression guard on the scan cost for N=50
    assert!(
        scan_cost < 15_000_000,
        "get_alerts_modified_since cost {scan_cost} exceeded upper bound 15M instructions"
    );
}

/// Confirms the fix for #38: a caller that pages with a small, bounded
/// `limit` pays a scan cost proportional to that `limit`, not to the
/// total number of alerts ever registered. This is what stops an
/// attacker from inflating `NEXT_ID` (via repeated `register_alert`
/// calls) from degrading the read path for every other caller — each
/// caller controls their own scan cost via `limit`.
#[test]
fn test_get_alerts_modified_since_pagination_bounds_scan_cost() {
    const N: u32 = 400;
    const PAGE: u32 = 10;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env, str(&env, "rule:transfer")];

    for _ in 0..N {
        let label = str(&env, "Alert");
        client.register_alert(&owner, &target, &label, &hash, &rules);
    }
    assert_eq!(client.get_alert_count(), u64::from(N));

    // A small page near the end of a large registry must still be cheap:
    // cost is bounded by PAGE, not by N.
    let cpu_before = env.cost_estimate().budget().cpu_instruction_cost();
    let page = client.get_alerts_modified_since(&0u64, &390u32, &PAGE);
    let cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let page_scan_cost = cpu_after.saturating_sub(cpu_before);

    assert_eq!(page.len() as usize, PAGE as usize);
    // A page of 10 out of a 400-alert registry should cost nowhere near
    // the ~15M-instruction ceiling asserted for a full 50-alert scan
    // above — if this ever regresses to an O(N) scan the cost will blow
    // well past this bound.
    assert!(
        page_scan_cost < 2_000_000,
        "paginated get_alerts_modified_since cost {page_scan_cost} exceeded upper bound 2M instructions for a page of {PAGE}"
    );

    // Requesting past the end of the registry returns an empty page
    // rather than scanning anything.
    let empty_page = client.get_alerts_modified_since(&0u64, &N, &PAGE);
    assert_eq!(empty_page.len(), 0);
}

/// Load test quantifying the repeated rescan cost in `assert_per_owner_limit` (#39).
/// Registers alerts with an active per-owner limit and benchmarks instruction growth,
/// asserting an upper bound regression guard.
/// Load test quantifying the (formerly O(n²)) cost of `assert_per_owner_limit` (#39).
///
/// Registers `LIMIT` alerts for the same owner under an active per-owner
/// limit and benchmarks instruction growth across the run. Before the
/// fix, `assert_per_owner_limit` rescanned `get_active_alert_count` (an
/// O(n) full-index scan) on every call, so `last_reg_cost` grew roughly
/// linearly with `LIMIT` — at LIMIT=100 the last call cost ~100x the
/// first. With the running per-owner counter, the limit check is O(1),
/// so cost per registration should stay flat regardless of `LIMIT`.
#[test]
fn test_load_assert_per_owner_limit_instruction_cost() {
    const LIMIT: u32 = 100;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_per_owner_alert_limit(&admin, &LIMIT);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env];

    let mut first_reg_cost: u64 = 0;
    let mut last_reg_cost: u64 = 0;

    let total_cpu_before = env.cost_estimate().budget().cpu_instruction_cost();

    for i in 0..LIMIT {
        let label = str(&env, "LimitLoadAlert");
        let before = env.cost_estimate().budget().cpu_instruction_cost();
        client.register_alert(&owner, &target, &label, &hash, &rules);
        let after = env.cost_estimate().budget().cpu_instruction_cost();
        let cost = after.saturating_sub(before);

        if i == 0 {
            first_reg_cost = cost;
        } else if i == LIMIT - 1 {
            last_reg_cost = cost;
        }
    }

    let total_cpu_after = env.cost_estimate().budget().cpu_instruction_cost();
    let total_registration_cost = total_cpu_after.saturating_sub(total_cpu_before);

    // Quantify that cost per registration includes the owner scan overhead
    assert!(
        first_reg_cost > 0 && last_reg_cost > 0,
        "Registration costs must be non-zero"
    );
    // Before/after regression guard: with an O(1) per-owner counter, the
    // Nth registration should not cost meaningfully more than the 1st.
    // (Under the old O(n) rescan, this ratio grew with LIMIT itself.)
    assert!(
        last_reg_cost < first_reg_cost.saturating_mul(3),
        "registration cost grew from {first_reg_cost} to {last_reg_cost} across {LIMIT} \
         calls — assert_per_owner_limit is no longer O(1)"
    );
    // Assert an upper bound regression guard on total batch registration cost with limit checks
    assert!(
        total_registration_cost < 50_000_000,
        "Total registration cost {total_registration_cost} exceeded upper bound 50M instructions"
    );
}

/// `get_non_removed_alert_count` is O(1) regardless of how many alerts an
/// owner has ever registered — it reads a maintained counter instead of
/// rescanning `OwnerIndex` (#39).
#[test]
fn test_get_non_removed_alert_count_instruction_cost_is_constant() {
    const N: u32 = 200;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);
    let hash = hash64(&env);
    let rules = vec![&env];

    for _ in 0..N {
        client.register_alert(&owner, &target, &str(&env, "Alert"), &hash, &rules);
    }

    let before = env.cost_estimate().budget().cpu_instruction_cost();
    let count = client.get_non_removed_alert_count(&owner);
    let after = env.cost_estimate().budget().cpu_instruction_cost();
    let cost = after.saturating_sub(before);

    assert_eq!(count, N);
    // An O(n) rescan at N=200 would cost far more than a single storage
    // read; this bound would fail under a scan-based implementation.
    assert!(
        cost < 200_000,
        "get_non_removed_alert_count cost {cost} at N={N} looks O(n), not O(1)"
    );
}

// #63 — ID monotonicity: each successive register_alert returns prev+1
#[test]
fn test_id_monotonicity() {
    const N: u64 = 10;

    let (env, client) = setup();

    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let mut prev_id: Option<u64> = None;
    for i in 0..N {
        let id = client.register_alert(
            &owner,
            &target,
            &str(&env, "alert"),
            &hash64(&env),
            &vec![&env],
        );
        if let Some(p) = prev_id {
            assert_eq!(
                id,
                p + 1,
                "expected id {} but got {} at iteration {}",
                p + 1,
                id,
                i
            );
        }
        prev_id = Some(id);
    }
}

// #33 — update_target_contract moves the alert to a new contract index
#[test]
fn test_update_target_contract() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let old_target = Address::generate(&env);
    let new_target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &old_target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    client.update_target_contract(&owner, &id, &new_target);

    // alert config reflects new target
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert_eq!(cfg.target_contract, new_target);

    // indexes updated correctly
    assert_eq!(client.get_alerts_for_contract(&owner, &old_target).len(), 0);
    assert_eq!(client.get_alerts_for_contract(&owner, &new_target).len(), 1);
}

// #33 — update_target_contract unauthorized
#[test]
fn test_update_target_contract_unauthorized() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);
    let new_target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(
        client
            .try_update_target_contract(&attacker, &id, &new_target)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

// 18. update_alert after remove_alert returns AlertNotFound
#[test]
fn test_update_alert_after_remove_returns_not_found() {
    let (env, client) = setup();
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

    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env], &false)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 19. update_webhook after remove_alert returns AlertNotFound
#[test]
fn test_update_webhook_after_remove_returns_not_found() {
    let (env, client) = setup();
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

    assert_eq!(
        client
            .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// 18. get_alert_active returns None for nonexistent ID
#[test]
fn test_get_alert_active_nonexistent() {
    let (env, client) = setup();
    assert!(client
        .get_alert_active(&Address::generate(&env), &999u64)
        .is_none());
}

// 19. get_alert_active returns true after registration
#[test]
fn test_get_alert_active_after_register() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
}

// 20. get_alert_active reflects update_alert changes
#[test]
fn test_get_alert_active_after_update() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));

    client.update_alert(&owner, &id, &vec![&env], &false);
    assert_eq!(client.get_alert_active(&owner, &id), Some(false));

    client.update_alert(&owner, &id, &vec![&env], &true);
    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
}

// 21. get_alert_active returns None after removal
#[test]
fn test_get_alert_active_after_remove() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.get_alert_active(&owner, &id), Some(true));
    client.remove_alert(&owner, &id);
    assert!(client.get_alert_active(&owner, &id).is_none());
}

// 21a. get_alert_owner returns the owner after registration
#[test]
fn test_get_alert_owner_after_register() {
    let (env, client) = setup();
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
        client.get_alert_owner(&owner, &id).unwrap(),
        Some(owner)
    );
}

// 21b. get_alert_owner reflects an accepted ownership transfer
#[test]
fn test_get_alert_owner_after_transfer() {
    let (env, client) = setup();
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

    client.propose_alert_transfer(&owner, &id, &new_owner);

    client.accept_alert_transfer(&new_owner, &id);
    assert_eq!(
        client.get_alert_owner(&owner, &id).unwrap(),
        Some(new_owner)
    );
}

// 21c. get_alert_owner returns None for nonexistent ID
#[test]
fn test_get_alert_owner_nonexistent() {
    let (env, client) = setup();
    assert!(client
        .get_alert_owner(&Address::generate(&env), &999u64)
        .is_none());
}

// 22. deactivate_all_alerts returns 0 when owner has no alerts
#[test]
fn test_deactivate_all_alerts_empty() {
    let (env, client) = setup();
    let owner = Address::generate(&env);

    assert_eq!(client.deactivate_all_alerts(&owner), 0);
}

// 23. deactivate_all_alerts deactivates all alerts for the owner
#[test]
fn test_deactivate_all_alerts_multiple() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );
    let id3 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 3"),
        &hash64c(&env, '3'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    assert_eq!(client.get_alert_active(&owner, &id1), Some(true));
    assert_eq!(client.get_alert_active(&owner, &id2), Some(true));
    assert_eq!(client.get_alert_active(&owner, &id3), Some(true));

    let count = client.deactivate_all_alerts(&owner);
    assert_eq!(count, 3);

    assert_eq!(client.get_alert_active(&owner, &id1), Some(false));
    assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
    assert_eq!(client.get_alert_active(&owner, &id3), Some(false));
}

// 24. deactivate_all_alerts only affects the calling owner's alerts
#[test]
fn test_deactivate_all_alerts_other_owner_unaffected() {
    let (env, client) = setup();
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner1,
        &target,
        &str(&env, "Owner1 Alert"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner2,
        &target,
        &str(&env, "Owner2 Alert"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    let count = client.deactivate_all_alerts(&owner1);
    assert_eq!(count, 1);

    assert_eq!(client.get_alert_active(&Address::generate(&env), &id1), Some(false));
    assert_eq!(client.get_alert_active(&Address::generate(&env), &id2), Some(true));
}

// 25. deactivate_all_alerts skips removed alerts and deactivates remaining
#[test]
fn test_deactivate_all_alerts_after_removal() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id1 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 1"),
        &hash64c(&env, '1'),
        &vec![&env, str(&env, "rule:transfer")],
    );
    let id2 = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert 2"),
        &hash64c(&env, '2'),
        &vec![&env, str(&env, "rule:mint")],
    );

    client.remove_alert(&owner, &id1);

    let count = client.deactivate_all_alerts(&owner);
    assert_eq!(count, 1);

    // id1 is gone
    assert!(client.get_alert(&owner, &id1).is_none());
    // id2 is now inactive
    assert_eq!(client.get_alert_active(&owner, &id2), Some(false));
}

// 18. get_alerts_by_owner_paginated — basic pagination
#[test]
fn test_get_alerts_by_owner_paginated() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for label in ["A", "B", "C", "D", "E"] {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env],
        );
    }

    // first page
    let page1 = client.get_alerts_by_owner_paginated(&owner, &owner, &0u32, &3u32);
    assert_eq!(page1.len(), 3);

    // second page
    let page2 = client.get_alerts_by_owner_paginated(&owner, &owner, &3u32, &3u32);
    assert_eq!(page2.len(), 2);

    // offset beyond length returns empty
    let empty = client.get_alerts_by_owner_paginated(&owner, &owner, &10u32, &3u32);
    assert_eq!(empty.len(), 0);
}

// 19. get_contract_alerts_paginated — basic pagination
#[test]
fn test_get_contract_alerts_paginated() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    for label in ["A", "B", "C", "D"] {
        client.register_alert(
            &owner,
            &target,
            &str(&env, label),
            &hash64(&env),
            &vec![&env],
        );
    }

    let page = client.get_contract_alerts_paginated(&owner, &target, &1u32, &2u32);
    assert_eq!(page.len(), 2);
}

// 18. get_admin panics with NotInitialized when contract is not initialized
// (Result-returning contract functions still panic via the plain client
// call when they return Err — this mirrors WatcherRegistry::get_admin.)
#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_admin_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    client.get_admin();
}

// 18b. get_admin returns a typed NotInitialized error via try_get_admin (#41)
#[test]
fn test_try_get_admin_uninitialized() {
    let env = Env::default();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);

    assert_eq!(
        client.try_get_admin().unwrap_err().unwrap(),
        ContractError::NotInitialized
    );
}

// 18c. get_admin returns Ok(admin) once initialized
#[test]
fn test_try_get_admin_after_initialize() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(client.try_get_admin().unwrap().unwrap(), admin);
    assert_eq!(client.get_admin(), admin);
}

// 19. Alert can be deactivated and reactivated via update_alert
#[test]
fn test_alert_deactivate_reactivate() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env, str(&env, "rule:mint")],
    );

    // deactivate
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &false)
            .unwrap(),
        Ok(())
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(!cfg.active);

    // reactivate
    assert_eq!(
        client
            .try_update_alert(&owner, &id, &vec![&env, str(&env, "rule:mint")], &true)
            .unwrap(),
        Ok(())
    );
    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(cfg.active);
}

// 18. update_webhook advances updated_at beyond its value at registration
#[test]
fn test_update_webhook_updates_timestamp() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id =
        client.register_alert(&owner, &target, &str(&env, "A"), &hash64(&env), &vec![&env]);

    let original_updated_at = client.get_alert(&owner, &id).unwrap().updated_at;
    env.ledger().set_timestamp(original_updated_at + 100);

    client
        .try_update_webhook(&owner, &id, &hash64c(&env, 'b'))
        .unwrap()
        .unwrap();

    let cfg = client.get_alert(&owner, &id).unwrap();
    assert!(cfg.updated_at > original_updated_at);
}

// 19. Multiple owners watching the same contract — indexes are isolated per owner
#[test]
fn test_multiple_owners_overlapping_target_contract() {
    let (env, client) = setup();
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);
    let target = Address::generate(&env);

    client.register_alert(
        &owner_a,
        &target,
        &str(&env, "Alert-A"),
        &hash64c(&env, '5'),
        &vec![&env],
    );
    client.register_alert(
        &owner_b,
        &target,
        &str(&env, "Alert-B"),
        &hash64c(&env, '6'),
        &vec![&env],
    );

    assert_eq!(client.get_alerts_for_contract(&owner_a, &target).len(), 2);

    let alerts_a = client.get_alerts_by_owner(&owner_a, &owner_a);
    assert_eq!(alerts_a.len(), 1);
    assert_eq!(alerts_a.get(0).unwrap().owner, owner_a);

    let alerts_b = client.get_alerts_by_owner(&owner_b, &owner_b);
    assert_eq!(alerts_b.len(), 1);
    assert_eq!(alerts_b.get(0).unwrap().owner, owner_b);
}

// B-1. bump_alert succeeds for an existing alert
#[test]
fn test_bump_alert_succeeds() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    assert_eq!(client.try_bump_alert(&id, &17_280u32).unwrap(), Ok(()));
}

// B-2. bump_alert returns AlertNotFound for a non-existent ID
#[test]
fn test_bump_alert_not_found() {
    let (_env, client) = setup();
    assert_eq!(
        client
            .try_bump_alert(&999u64, &17_280u32)
            .unwrap_err()
            .unwrap(),
        ContractError::AlertNotFound
    );
}

// B-3. bump_alert clamps TTL above MAX_TTL to MAX_TTL
#[test]
fn test_bump_alert_clamps_to_max_ttl() {
    use soroban_sdk::testutils::Events as _;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    // Request a TTL above the protocol maximum
    client.bump_alert(&id, &u32::MAX);

    // The emitted event should carry the clamped effective TTL
    let events = env.events().all();
    let bump_event = events.iter().find(|(_, topics, _)| {
        topics.len() == 2
            && Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("alert")
            && Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("bump")
    });
    assert!(bump_event.is_some(), "expected an alert.bump event");

    let (_, _, data) = bump_event.unwrap();
    let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_id, id);
    assert_eq!(emitted_ttl, MAX_TTL, "TTL must be clamped to MAX_TTL");
}

// B-4. bump_alert with TTL below MAX_TTL uses the requested value exactly
#[test]
fn test_bump_alert_uses_requested_ttl_when_below_max() {
    use soroban_sdk::testutils::Events as _;

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    let requested_ttl: u32 = 120_960; // ~7 days, well below MAX_TTL
    client.bump_alert(&id, &requested_ttl);

    let events = env.events().all();
    let bump_event = events.iter().find(|(_, topics, _)| {
        topics.len() == 2
            && Symbol::from_val(&env, &topics.get(0).unwrap())
                == soroban_sdk::symbol_short!("alert")
            && Symbol::from_val(&env, &topics.get(1).unwrap())
                == soroban_sdk::symbol_short!("bump")
    });
    assert!(bump_event.is_some());

    let (_, _, data) = bump_event.unwrap();
    let (_, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_ttl, requested_ttl);
}

// B-5. bump_alert emits the correct event shape (topic + data)
#[test]
fn test_bump_alert_event_shape() {
    use soroban_sdk::{symbol_short, testutils::Events as _};

    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Alert"),
        &hash64(&env),
        &vec![&env],
    );

    let ttl: u32 = 17_280;
    client.bump_alert(&id, &ttl);

    let events = env.events().all();
    let bump_event = events
        .iter()
        .find(|(_, topics, _)| {
            topics.len() == 2
                && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("alert")
                && Symbol::from_val(&env, &topics.get(1).unwrap()) == symbol_short!("bump")
        })
        .expect("alert.bump event must be emitted");

    // Verify data shape: (id: u64, ttl: u32)
    let (_, _, data) = bump_event;
    let (emitted_id, emitted_ttl): (u64, u32) = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_id, id);
    assert_eq!(emitted_ttl, ttl);
}

// B-6. bump_alert does not modify the alert's content
#[test]
fn test_bump_alert_does_not_modify_content() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(
        &owner,
        &target,
        &str(&env, "Immutable"),
        &hash64c(&env, 'c'),
        &vec![&env, str(&env, "rule:transfer")],
    );

    let before = client.get_alert(&owner, &id).unwrap();
    client.bump_alert(&id, &17_280u32);
    let after = client.get_alert(&owner, &id).unwrap();

    // All fields must be identical after a bump
    assert_eq!(after.label, before.label);
    assert_eq!(after.webhook_hash, before.webhook_hash);
    assert_eq!(after.rules.len(), before.rules.len());
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.target_contract, before.target_contract);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.active, before.active);
}

// B-7. DEFAULT_TTL and MAX_TTL constants have the expected values
#[test]
fn test_ttl_constants() {
    // MAX_TTL must be strictly greater than DEFAULT_TTL (checked at compile time)
    const _: () = assert!(MAX_TTL > DEFAULT_TTL);

    // DEFAULT_TTL ≈ 24 hours at 5 s/ledger
    assert_eq!(DEFAULT_TTL, 17_280);
    // MAX_TTL ≈ 31 days at 5 s/ledger
    assert_eq!(MAX_TTL, 535_680);
}
