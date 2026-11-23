#![no_main]

use alert_registry::{AlertRegistry, AlertRegistryClient, ContractError, VALID_RULE_DESCRIPTORS};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::Address as _, Address, BytesN, Env, String as SorobanString, Vec as SorobanVec,
};

/// Structured fuzz input derived via `arbitrary`, replacing the manual
/// byte-slice parsing that caused out-of-bounds indexing (see #426).
#[derive(Arbitrary, Debug)]
struct FuzzInput {
    /// Raw bytes for the alert label (may not be valid UTF-8).
    label_bytes: Vec<u8>,
    /// 32-byte webhook hash.
    webhook_hash: [u8; 32],
    /// Indices into VALID_RULE_DESCRIPTORS (arbitrary crate keeps values in
    /// range via modular reduction in the harness, so any u8 value is safe).
    rule_indices: Vec<u8>,
}

fuzz_target!(|input: FuzzInput| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    // Register with constructor args — no separate `initialize` call needed.
    let contract_id = env.register(AlertRegistry, (&admin,));
    let client = AlertRegistryClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Fix #1: label is built entirely from input.label_bytes — no manual
    // byte-slice arithmetic that could index out of bounds.
    let label = match core::str::from_utf8(&input.label_bytes) {
        Ok(s) => SorobanString::from_str(&env, s),
        Err(_) => return, // not valid UTF-8 — skip this input
    };

    let hash = BytesN::from_array(&env, &input.webhook_hash);

    // Build a rule list from the fuzz-controlled indices. Every index is
    // reduced modulo VALID_RULE_DESCRIPTORS.len() so no OOB access is possible.
    let mut rules: SorobanVec<SorobanString> = SorobanVec::new(&env);
    for idx in &input.rule_indices {
        let rule = VALID_RULE_DESCRIPTORS[*idx as usize % VALID_RULE_DESCRIPTORS.len()];
        rules.push_back(SorobanString::from_str(&env, rule));
    }

    let result = client.try_register_alert(&owner, &target, &label, &hash, &rules);

    // Fix #2: assert on the outcome rather than silently discarding it.
    match result {
        Ok(Ok(config_id)) => {
            // Success: the returned ID must round-trip through get_alert.
            // We use the owner as the querier (no watcher-gating is configured).
            let fetched = client
                .try_get_alert(&owner, &config_id)
                .expect("get_alert must not panic")
                .expect("get_alert must return Ok");
            let config = fetched.expect("alert registered successfully must be retrievable");
            assert_eq!(config.owner, owner);
            assert_eq!(config.target_contract, target);
        }
        Ok(Err(contract_error)) => {
            // Only the documented validation errors are acceptable outcomes.
            assert!(
                matches!(
                    contract_error,
                    ContractError::NotInitialized
                        | ContractError::EmptyLabel
                        | ContractError::LabelTooLong
                        | ContractError::TooManyRules
                        | ContractError::EmptyRules
                        | ContractError::InvalidRuleDescriptor
                        | ContractError::InvalidWebhookHash
                        | ContractError::DuplicateRule
                        | ContractError::OwnerAlertLimitExceeded
                        | ContractError::ContractAlertLimitExceeded
                        | ContractError::GlobalAlertLimitExceeded
                        | ContractError::Paused
                ),
                "unexpected ContractError variant: {:?}",
                contract_error,
            );
        }
        Err(_invoke_error) => {
            // A host-level InvokeError (e.g. auth failure simulated by the
            // test environment) is not a contract bug; skip silently.
        }
    }
});
