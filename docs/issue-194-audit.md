# Issue #194 Audit — Closed Issues Whose Fixes Are Not on `main`

Audited all closed issues #2–#126 against the code on `main` (commit
`2e82d80`). The table below records each issue, the PR that closed it,
and whether the fix is actually present in the codebase.

## Summary

| Issue | Title | Closed by PR | Fix on `main`? |
|-------|-------|-------------|----------------|
| #21 | Make the Per-Owner Alert Limit Reflect Active Alerts | #172 | ❌ No |
| #22 | Enforce the Per-Owner Alert Limit on Reactivation | #172 | ❌ No |
| #24 | Validate webhook hash length | #314 | ✅ Yes (`BytesN<32>`) |
| #27 | Reject No-Op Retargets in `update_target_contract` | #173 | ❌ No (docs only) |
| #28 | Make `bump_alert` Error on Over-Limit TTL | #173 | ❌ No (docs only) |
| #75 | Add CI job to publish `@tx-wat/alert-registry-bindings` | #146 | ❌ No (docs only) |
| #81 | Add npm Provenance to Published Bindings | #129 | ❌ No (test suite only) |
| #86 | `verify.sh` uses unoptimized WASM | #313 | ✅ Yes (`wasm_path()` → `.optimized.wasm`) |
| #87 | `upgrade.sh` uses unoptimized WASM | #313 | ✅ Yes (`build_contract` optimizes) |

---

## Unresolved Issues — Detail

### #21 — Per-owner limit counts non-removed alerts, not active ones
**Closed by:** PR #172 (`fix(alert-registry): count only active alerts in get_active_alert_count`)  
**Why still broken:** PR #172 fixed `get_active_alert_count` to scan the owner index and count only `active == true` entries. But `assert_per_owner_limit` still calls `get_non_removed_alert_count` (the O(1) live counter), not `get_active_alert_count`. Deactivating alerts does not free quota.

**Fix needed:**
```rust
// contracts/alert-registry/src/lib.rs  — assert_per_owner_limit
fn assert_per_owner_limit(env: &Env, owner: &Address) -> Result<(), ContractError> {
    let limit = Self::get_per_owner_alert_limit(env.clone());
-   if limit > 0 && Self::get_non_removed_alert_count(env.clone(), owner.clone()) >= limit {
+   if limit > 0 && Self::get_active_alert_count(env.clone(), owner.clone()) >= limit {
        return Err(ContractError::OwnerAlertLimitExceeded);
    }
    Ok(())
}
```

**Regression test required:** deactivate all alerts, confirm a new `register_alert` succeeds.

---

### #22 — `update_alert(active=true)` bypasses the per-owner limit
**Closed by:** PR #172  
**Why still broken:** `update_alert` makes no call to `assert_per_owner_limit`. An owner at the cap can deactivate an alert and immediately reactivate it (or reactivate a previously deactivated alert) to hold more active alerts than the configured limit allows.

**Fix needed:**
```rust
// contracts/alert-registry/src/lib.rs  — update_alert
pub fn update_alert(env, caller, config_id, rules, active) -> Result<(), ContractError> {
    // ... existing auth + fetch + owner check ...

+   // Enforce the per-owner active limit when re-activating an alert.
+   if active && !config.active {
+       Self::assert_per_owner_limit(&env, &caller)?;
+   }

    config.rules = rules;
    config.active = active;
    // ...
}
```

**Regression test required:** fill quota → deactivate one → attempt `update_alert(active=true)` → expect `OwnerAlertLimitExceeded`.

---

### #27 — `update_target_contract` accepts no-op same-address retargets
**Closed by:** PR #173 (`docs(alert-registry): record config-time watcher-registry validation in threat model`)  
**Why still broken:** PR #173 made docs-only changes. `update_target_contract` performs a full storage write + index remove + index push + TTL refresh even when `new_target == config.target_contract`.

**Fix needed:**
```rust
// contracts/alert-registry/src/lib.rs  — update_target_contract
pub fn update_target_contract(env, caller, config_id, new_target) -> Result<(), ContractError> {
    // ... auth + fetch + owner check ...

+   if new_target == config.target_contract {
+       return Ok(()); // or: return Err(ContractError::SameTarget);
+   }

    let old_target = config.target_contract.clone();
    // ...
}
```

**Regression test required:** call `update_target_contract` with same address and assert no index corruption / no spurious event emitted.

---

### #28 — `bump_alert` silently clamps over-limit TTL instead of returning an error
**Closed by:** PR #173  
**Why still broken:** PR #173 made docs-only changes. `bump_alert` still uses `ttl.min(MAX_TTL)` silently. The clamped effective TTL is reported in the `alert.bump` event, but no error is returned, inconsistent with the contract's general convention of erroring on invalid input (e.g. `LabelTooLong`, `TooManyRules`).

**Decision needed:** either return `Err(ContractError::TtlTooLarge)` when `ttl > MAX_TTL` (breaking), or explicitly document clamping as the intended behaviour in the function's doc comment and the `MAX_TTL` constant doc (keeping existing clamping but resolving the ambiguity).

**Regression test required:** call `bump_alert` with `ttl = MAX_TTL + 1` and assert the expected outcome (error or clamped event value).

---

### #75 — `publish-bindings.yml` does not publish `@tx-wat/alert-registry-bindings`
**Closed by:** PR #146 (`docs: document the 6 undocumented WatcherRegistry functions (#64)`)  
**Why still broken:** PR #146 made docs-only changes to `docs/watcher-registry.md`. `publish-bindings.yml` has a single job that generates and publishes only `@tx-wat/watcher-registry`. There is no corresponding job for `@tx-wat/alert-registry-bindings`, yet `README.md` and `bindings/alert-registry/README.md` both claim it is published by CI on the first tagged release.

**Fix needed:** add a second job to `publish-bindings.yml` (or a second step block) that mirrors the existing watcher-registry job for alert-registry, including:
- CHANGELOG guard
- WASM build
- `stellar contract bindings typescript --wasm alert_registry.wasm`
- package.json overlay (`name: @tx-wat/alert-registry-bindings`)
- `npm run build && npm test`
- `npm publish --access public --provenance` (see #81)

---

### #81 — `npm publish` in `publish-bindings.yml` has no `--provenance`
**Closed by:** PR #129 (`test: add minimal test suite for TypeScript bindings`)  
**Why still broken:** PR #129 added a vitest test suite to both binding packages. It did not touch `publish-bindings.yml`. The `npm publish` step still runs without `--provenance` and without `id-token: write` permission.

**Fix needed:**
```yaml
# .github/workflows/publish-bindings.yml
    permissions:
      contents: read
+     id-token: write   # required for npm provenance

      - name: Publish to npm
        run: npm publish --access public --provenance
```

---

## Re-opening Upstream Issues (maintainer action required)

The fork's Codespaces token does not have write access to `Tx-wats/contracts`.
A maintainer with access must re-open the issues. With a PAT that has `repo`
scope for `Tx-wats/contracts`:

```bash
for N in 21 22 27 28 75 81; do
  gh api --method PATCH /repos/Tx-wats/contracts/issues/$N -f state=open
  echo "Re-opened #$N"
done
```

Suggested comment to post on each (replace `N` and body as appropriate):

```bash
gh api --method POST /repos/Tx-wats/contracts/issues/22/comments \
  -f body="Re-opening: fix not on main. PR #172 closed this but update_alert still does not call assert_per_owner_limit when active transitions false → true. See docs/issue-194-audit.md for the exact code change needed."
```
