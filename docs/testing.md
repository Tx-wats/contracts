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

## Auth-Required Test Coverage Checklist

Every state-mutating public function in both contracts has been audited and equipped with a dedicated regression test confirming `require_auth()` is strictly enforced.

### AlertRegistry

| Function | Required Signer | Auth Required Test | Status |
|---|---|---|:---:|
| `register_alert` | `owner` | `test_register_alert_requires_auth` | ✅ |
| `update_alert` | `caller` (owner) | `test_update_alert_requires_auth` | ✅ |
| `update_webhook` | `caller` (owner) | `test_update_webhook_requires_auth` | ✅ |
| `propose_webhook` | `caller` (owner) | `test_propose_webhook_requires_auth` | ✅ |
| `confirm_webhook` | `caller` (owner) | `test_confirm_webhook_requires_auth` | ✅ |
| `renew_alert_ttl` | `caller` (owner) | `test_renew_alert_ttl_requires_auth` | ✅ |
| `update_label` | `caller` (owner) | `test_update_label_requires_auth` | ✅ |
| `update_target_contract` | `caller` (owner) | `test_update_target_contract_requires_auth` | ✅ |
| `deactivate_all_alerts` | `caller` (owner) | `test_deactivate_all_alerts_requires_auth` | ✅ |
| `remove_alert` | `caller` (owner) | `test_remove_alert_requires_auth` | ✅ |
| `remove_alert_by_admin` | `admin` | `test_remove_alert_by_admin_requires_auth` | ✅ |
| `transfer_admin` | `admin` | `test_transfer_admin_requires_auth` | ✅ |
| `set_per_owner_alert_limit` | `admin` | `test_set_per_owner_alert_limit_requires_auth` | ✅ |
| `set_watcher_registry` | `admin` | `test_set_watcher_registry_requires_auth` | ✅ |
| `bump_alert` | *None* (permissionless keeper) | *Tested via `test_bump_alert_by_third_party`* | N/A |
| `initialize` | *None* (first-caller bootstrap) | *Tested via `test_double_initialize`* | N/A |

### WatcherRegistry

| Function | Required Signer | Auth Required Test | Status |
|---|---|---|:---:|
| `initialize` | `admin` | `test_initialize_requires_admin_auth` | ✅ |
| `add_admin` | `caller` (admin) | `test_add_admin_requires_auth` | ✅ |
| `remove_admin` | `caller` (admin) | `test_remove_admin_requires_auth` | ✅ |
| `transfer_admin` | `admin` | `test_transfer_admin_requires_auth` | ✅ |
| `register_watcher` | `admin` | `test_register_watcher_requires_auth` | ✅ |
| `remove_watcher` | `admin` | `test_remove_watcher_requires_auth` | ✅ |
| `replace_watcher` | `admin` | `test_replace_watcher_requires_auth` | ✅ |
| `clear_all_watchers` | `admin` | `test_clear_all_watchers_requires_auth` | ✅ |

---

## Property-Based Testing (proptest)

In addition to traditional unit tests, `contracts/alert-registry` includes property-based tests powered by `proptest` (`contracts/alert-registry/src/proptests.rs`).

### Invariants Verified

1. **State Machine Invariants (`proptest_alert_state_machine_sequences`)**:
   - Random sequences of register, update, propose/confirm webhook, renew TTL, update label, update target contract, deactivate all, remove, remove by admin, and bump alert calls.
   - **Removed alert irreversibility**: A removed alert can never be read, reactivated, modified, or confirmed (`AlertNotFound` on all subsequent attempts).
   - **Monotonic ID counter**: Global alert ID sequence matches total registered count.
   - **Index synchronization**: Owner index and contract index counts strictly match live alerts.
   - **Timestamp progression**: `updated_at >= created_at` across all state transitions.

2. **Two-Phase Webhook Rotation (`proptest_webhook_rotation_lifecycle`)**:
   - `propose_webhook` sets `pending_webhook_hash` without changing the live `webhook_hash` or `updated_at`.
   - Repeated proposals overwrite `pending_webhook_hash` without affecting live state.
   - `confirm_webhook` atomically promotes `pending_webhook_hash` to `webhook_hash` and clears pending state.
   - Calling `confirm_webhook` without a pending proposal returns `ContractError::NoPendingWebhook`.

3. **Permissionless TTL Clamping (`proptest_bump_alert_ttl_clamping`)**:
   - `bump_alert` succeeds permissionlessly for any requested TTL and clamps to `MAX_TTL`.

4. **Pagination Bounds (`proptest_pagination_bounds_and_ordering`)**:
   - Slicing bounds and offset limits always preserve FIFO ordering and never panic on out-of-bounds offsets.

5. **Timestamp Query Filtering (`proptest_modified_since_filtering`)**:
   - `get_alerts_modified_since` returns exactly the subset of live alerts with `updated_at >= since`.

---

## Mutation Testing (`cargo-mutants`)

Mutation testing validates the fault-catching power of the test suite by deliberately injecting small syntactic and logical mutations (e.g. replacing `==` with `!=`, mutating return values to defaults, removing statements) and verifying that at least one test fails.

### Running Mutation Tests

Run mutation testing across both contract crates:

```bash
# Run via script
./scripts/run-mutants.sh [watcher-registry | alert-registry | all]

# Or directly with cargo
cargo mutants --in-place -p watcher-registry
cargo mutants --in-place -p alert-registry
```

### Configuration (`.cargo/mutants.toml`)

Mutation scope is configured in `.cargo/mutants.toml`:

```toml
examine_globs = [
    "contracts/alert-registry/src/lib.rs",
    "contracts/watcher-registry/src/lib.rs",
]

exclude_globs = [
    "contracts/alert-registry/src/tests.rs",
    "contracts/alert-registry/src/proptests.rs",
    "contracts/watcher-registry/src/tests.rs",
    "contracts/integration-tests/**",
    "contracts/test-utils/**",
    "**/tests/**",
]

minimum_test_timeout = 30
timeout_multiplier = 2
```

### Mutation Analysis & Baseline Results

| Package | Total Mutants | Caught | Unviable / Non-compiling | Missed / Surviving | Mutant Kill Rate |
|---|---|---|---|---|:---:|
| `watcher-registry` | 44 | 35 | 9 | 0 | **100%** |
| `alert-registry` | 102 | 87 | 15 | 0 | **100%** |

### High-Value Mutants Identified & Killed

During the mutation testing audit, dedicated killer unit tests were added to guarantee no subtle logic gaps survive:
- **Per-Owner Limit Exact Boundary**: Enforces boundary checks (`>= limit`) on registration and limit adjustments.
- **Rule Descriptor Validation**: Verified rejection of non-whitelisted descriptors (`"rule:burn"`, empty strings).
- **Target Contract Index Relocation**: Verifies that updating a target contract removes the alert from the old contract's index and appends it to the new contract's index.
- **Timestamp Filter Precision**: Verifies exact inclusive boundary behavior on `get_alerts_modified_since`.
- **Pagination Offsets**: Verifies empty return when offset exceeds total elements.


