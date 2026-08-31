use crate::AlertInput;
use crate::AlertRegistry;
use crate::AlertRegistryClient;
use crate::ContractError;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    vec, Address, Env, String, Vec,
};

fn setup() -> (Env, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client)
}

/// A 64-character webhook hash of repeated `c` — `register_alert`,
/// `update_webhook` and `propose_webhook` all require exactly 64 characters.
fn hash64c(env: &Env, c: char) -> String {
    let buf = [c as u8; 64];
    String::from_str(env, core::str::from_utf8(&buf).unwrap())
}

/// The default valid 64-character webhook hash.
fn hash64(env: &Env) -> String {
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
    // webhook_hash must be exactly 64 characters
    let webhook_hash = str(
        &env,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
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

// ── Issue #34 — transfer_alert_ownership ────────────────────────────────

#[test]
fn test_transfer_alert_ownership_success() {
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

    client.transfer_alert_ownership(&owner, &id, &new_owner);

    let cfg = client.get_alert(&id).unwrap();
    assert_eq!(cfg.owner, new_owner);

    // OwnerIndex updated for both the old and new owner.
    assert_eq!(client.get_alerts_by_owner(&new_owner, &owner).len(), 0);
    let new_owner_alerts = client.get_alerts_by_owner(&new_owner, &new_owner);
    assert_eq!(new_owner_alerts.len(), 1);
    assert_eq!(new_owner_alerts.get(0).unwrap().owner, new_owner);
}

#[test]
fn test_transfer_alert_ownership_unauthorized() {
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
            .try_transfer_alert_ownership(&attacker, &id, &new_owner)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    // Ownership and indexes are unchanged.
    assert_eq!(client.get_alert(&id).unwrap().owner, owner);
    assert_eq!(client.get_alerts_by_owner(&owner, &owner).len(), 1);
}

#[test]
fn test_transfer_alert_ownership_not_found() {
    let (env, client) = setup();
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

    client.transfer_alert_ownership(&owner, &id, &new_owner);
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

    let mut bad = alert_input(&env, &owner, &target, "Bad");
    bad.webhook_hash = str(&env, "too-short");

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
        ContractError::InvalidWebhookHash
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
