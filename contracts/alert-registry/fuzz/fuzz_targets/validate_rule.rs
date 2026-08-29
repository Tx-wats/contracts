#![no_main]

use alert_registry::AlertRegistry;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{vec, Env, String as SorobanString, Vec as SorobanVec};

fuzz_target!(|data: &[u8]| {
    // 1. Fuzz single rule descriptor validation with arbitrary byte slices and UTF-8 strings
    if let Ok(s) = core::str::from_utf8(data) {
        let env = Env::default();
        let soroban_str = SorobanString::from_str(&env, s);

        let res = AlertRegistry::validate_rule(&env, &soroban_str);

        // Invariant: only "rule:transfer" and "rule:mint" are valid rule descriptors.
        // All other strings (empty, prefix mismatches, unusual bytes, format-strings, etc.)
        // must be rejected with InvalidRuleDescriptor without panicking.
        if s == "rule:transfer" || s == "rule:mint" {
            assert!(
                res.is_ok(),
                "Valid rule descriptor must succeed: {}",
                s
            );
        } else {
            assert!(
                res.is_err(),
                "Invalid rule descriptor must be rejected: {}",
                s
            );
        }

        // 2. Fuzz vector validation with the generated rule descriptor
        let mut rules: SorobanVec<SorobanString> = vec![&env];
        rules.push_back(soroban_str);
        let rules_res = AlertRegistry::validate_rules(&env, &rules);

        if s == "rule:transfer" || s == "rule:mint" {
            assert!(rules_res.is_ok());
        } else {
            assert!(rules_res.is_err());
        }
    }
});
