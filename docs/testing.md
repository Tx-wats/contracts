# Testing Guide

## Running Tests

```bash
cargo test
# or
make test
```

## Test Setup Pattern

Both contracts use the same setup helper pattern:

```rust
fn setup() -> (Env, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AlertRegistry);
    let client = AlertRegistryClient::new(&env, &contract_id);
    (env, client)
}
```

### What `env.mock_all_auths()` does

In the Soroban test environment, every call to `address.require_auth()` inside a contract will **panic** unless auth has been satisfied. `mock_all_auths()` tells the test environment to automatically approve every auth check for any address — it bypasses the need to construct and sign real Stellar transactions in unit tests.

This is appropriate for happy-path tests where you want to verify business logic without dealing with cryptographic signing overhead.

**It does not skip the ownership checks you write yourself.** For example, this contract code:

```rust
caller.require_auth();          // mocked — passes
if config.owner != caller {     // your logic — still enforced
    panic!("unauthorized");
}
```

`mock_all_auths()` satisfies `require_auth()`, but the `owner != caller` guard runs normally. Unauthorized-caller tests still panic as expected.

## Verifying Auth Is Actually Required

To confirm that `require_auth()` is enforced on-chain (i.e., not accidentally removed), write a test **without** `mock_all_auths()` and expect a panic:

```rust
#[test]
#[should_panic]
fn test_register_alert_requires_auth() {
    let env = Env::default();
    // No mock_all_auths() — auth checks are enforced
    let contract_id = env.register_contract(None, AlertRegistry);
    let client = AlertRegistryClient::new(&env, &contract_id);

    let owner = Address::generate(&env);
    let target = Address::generate(&env);

    // This will panic because owner.require_auth() is not satisfied
    client.register_alert(
        &owner,
        &target,
        &String::from_str(&env, "Alert"),
        &String::from_str(&env, "hash"),
        &vec![&env],
    );
}
```

If this test stops panicking, `require_auth()` has been removed from the function — a security regression.

## Unauthorized Caller Tests

These tests use `mock_all_auths()` (so `require_auth()` passes) but pass a different address as the caller to trigger the ownership guard:

```rust
#[test]
#[should_panic(expected = "unauthorized")]
fn test_update_unauthorized() {
    let (env, client) = setup();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let id = client.register_alert(&owner, &target, ...);

    // attacker passes require_auth() (mocked) but fails the owner check
    client.update_alert(&attacker, &id, &vec![&env], &false);
}
```

## Regression Testing

Each contract contains a dedicated `regression_tests` module (`contracts/alert-registry/src/regression_tests.rs` and `contracts/watcher-registry/src/regression_tests.rs`) tied directly to historical bugs documented in `CHANGELOG.md`.

These tests run automatically on every `cargo test` execution and in CI to guarantee that resolved correctness bugs, missing function bodies, event emission oversights, or parameter boundary errors never resurface.

## Fuzz Testing

The `alert-registry` contract includes `cargo-fuzz` targets for fuzz testing rule-descriptor parsing and validation (`contracts/alert-registry/fuzz`).

### Running Fuzz Tests

```bash
# Run the validate_rule fuzz target for 30 seconds
cd contracts/alert-registry
cargo +nightly fuzz run validate_rule -- -max_total_time=30
```

The fuzz target subjects `AlertRegistry::validate_rule` and `AlertRegistry::validate_rules` to random byte streams, arbitrary UTF-8 strings, length boundaries, format-string-like patterns (`%s`, `%x`, `%n`), and invalid prefixes, verifying invariant enforcement and absence of panics. See [Fuzz Testing Findings](fuzzing-findings.md) for full execution results and coverage data.

## Summary

| Test type | Use `mock_all_auths()`? | What it verifies |
|---|---|---|
| Happy path | Yes | Business logic works correctly |
| Unauthorized caller | Yes | Ownership guards reject wrong callers |
| Auth required | No | `require_auth()` is present and enforced |
| Regression tests | Yes / Context-dependent | Historical bugs in CHANGELOG.md cannot resurface |
| Fuzz tests | N/A (libFuzzer) | Rule-descriptor parsing robustness under arbitrary inputs |

