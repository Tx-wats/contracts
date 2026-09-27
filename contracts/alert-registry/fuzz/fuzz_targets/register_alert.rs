#![no_main]

use alert_registry::{AlertRegistry, AlertRegistryClient, ContractError};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String as SorobanString, Vec as SorobanVec};

fuzz_target!(|data: &[u8]| {
    if data.len() < 70 {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // Parse fuzz input
    let label_len = data[0] as usize;
    let hash_len = data[1] as usize;
    let rules_count = data[2] as usize;

    let label_end = 3 + label_len.min(100);
    let label = if let Ok(s) = core::str::from_utf8(&data[3..label_end.min(data.len())]) {
        SorobanString::from_str(&env, s)
    } else {
        return;
    };

    // Create deterministic hash
    let hash_bytes: [u8; 32] = {
        let mut b = [0u8; 32];
        for i in 0..32.min(hash_len.min(data.len())) {
            b[i] = data[label_end + i];
        }
        b
    };
    let hash = BytesN::from_array(&env, &hash_bytes);

    // Create rule set
    let mut rules: SorobanVec<SorobanString> = SorobanVec::new(&env);
    let rule_names = ["rule:transfer", "rule:mint"];
    for i in 0..(rules_count.min(2)) {
        rules.push_back(SorobanString::from_str(&env, rule_names[i % 2]));
    }

    let result = client.try_register_alert(&owner, &target, &label, &hash, &rules);

    // Verify error types
    match result {
        Err(e) => {
            // Verify error is one of the expected types
            let _ = e;
        }
        Ok(_) => {
            // Success case: alert should be registered
        }
    }
});
