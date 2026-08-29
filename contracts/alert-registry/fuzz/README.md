# Fuzz Testing for AlertRegistry

This directory contains `cargo-fuzz` (libFuzzer) targets for `alert-registry` rule-descriptor parsing and validation.

## Fuzz Targets

### `validate_rule`
- **Target Source**: `fuzz_targets/validate_rule.rs`
- **Code Tested**: `AlertRegistry::validate_rule` and `AlertRegistry::validate_rules` in `contracts/alert-registry/src/lib.rs`
- **Test Invariants**:
  - Valid descriptors (`"rule:transfer"`, `"rule:mint"`) are accepted (`Ok(())`).
  - All other random strings, malformed prefixes, oversized buffers, format-string-like payloads (`%s`, `%x`, `%n`), control characters, and unusual byte sequences are rejected with `ContractError::InvalidRuleDescriptor`.
  - Vectors exceeding 50 rules are rejected with `ContractError::TooManyRules`.
  - No panics, buffer overflows, or unexpected crashes occur on arbitrary input.

## Running the Fuzz Target

### Prerequisites
- Rust Nightly toolchain: `rustup toolchain install nightly`
- `cargo-fuzz`: `cargo install cargo-fuzz --locked`

### Execution

Run for a bounded duration (e.g. 30 seconds):
```bash
cd contracts/alert-registry
cargo +nightly fuzz run validate_rule -- -max_total_time=30
```

Run with a bounded number of iterations (e.g. 100,000 runs):
```bash
cd contracts/alert-registry
cargo +nightly fuzz run validate_rule -- -runs=100000
```
