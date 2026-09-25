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

        // A one-item collection must have the same validation result as its
        // only descriptor. The descriptor list itself is owned by the
        // contract, so this oracle does not duplicate that list.
        let mut rules: SorobanVec<SorobanString> = vec![&env];
        rules.push_back(soroban_str);
        let rules_res = AlertRegistry::validate_rules(&env, &rules);
        assert_eq!(res.is_ok(), rules_res.is_ok());
    }
});
