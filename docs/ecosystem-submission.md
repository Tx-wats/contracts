# Ecosystem Submission Guide

This document tracks the submission status of `stellar-txwatch-contracts` to the
Stellar Developer Tools ecosystem listing and the Soroban example contracts
repository.

---

## Submission Status

| Target | Status | Submission Date | PR / Tracking Link | Outcome / Notes |
|---|---|---|---|---|
| **Stellar Developer Tools Listing** (`stellar/stellar-docs`) | `Pending Submission` | — | — | Ready for PR submission to `stellar/stellar-docs` under Smart Contracts / Monitoring |
| **Soroban Example Contracts** (`stellar/soroban-examples`) | `Pending Submission` | — | — | Ready for PR submission to `stellar/soroban-examples` once v0.1.0 release is tagged |
| **npm Bindings Publishing** (`@tx-wat/watcher-registry`) | `Configured` | 2025-05-28 | [`.github/workflows/publish-bindings.yml`](../.github/workflows/publish-bindings.yml) | Automated CI publish on tagged release |

> **Status Values:**
> - `Pending Submission`: Ready to be submitted, awaiting submission PR creation.
> - `Under Review`: PR submitted, awaiting maintainer review/merge.
> - `Accepted` / `Merged`: PR merged and live in upstream documentation/repository.
> - `Rejected` / `Revision Requested`: Feedback received requiring changes.

---

## 1. Stellar Developer Tools Ecosystem Listing

The [Stellar Developer Tools](https://developers.stellar.org/tools) page lists
community-built tools, libraries, and SDKs. Submissions are made via pull
request to the
[stellar/stellar-docs](https://github.com/stellar/stellar-docs) repository.

### Submission entry

The entry below is ready to be added to the appropriate section of
`docs/tools/developer-tools.mdx` (or the current equivalent file) in the
`stellar/stellar-docs` repository.

```mdx
### stellar-txwatch-contracts

On-chain Soroban smart contracts for alert configuration storage and watcher
node authorization, part of the [Tx-wat](https://github.com/Tx-wat) monitoring
ecosystem.

| Property | Value |
|---|---|
| **Repository** | https://github.com/Tx-wat/stellar-txwatch-contracts |
| **npm (TypeScript bindings)** | `@tx-wat/watcher-registry` |
| **Network** | Testnet · Mainnet |
| **Language** | Rust (Soroban SDK 22) |
| **License** | MIT |

**Contracts**

- **AlertRegistry** — stores alert configurations on-chain keyed by contract
  address. Supports per-owner limits, paginated queries, and admin controls.
- **WatcherRegistry** — stores authorized watcher node addresses with a
  multi-admin model and on-chain audit events.

**Links**

- [Function reference — Alert Registry](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/docs/alert-registry.md)
- [Function reference — Watcher Registry](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/docs/watcher-registry.md)
- [Deployed addresses](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/DEPLOYMENTS.md)
```

### How to submit

1. Fork [stellar/stellar-docs](https://github.com/stellar/stellar-docs).
2. Locate the developer tools listing file (currently
   `docs/tools/developer-tools.mdx` or the community tools section).
3. Add the MDX block above under the **Smart Contracts / Monitoring** category
   (create the category if it does not exist).
4. Open a pull request with the title:
   `feat(tools): add stellar-txwatch-contracts to ecosystem listing`
5. Fill in the PR description referencing this file and the project README.

---

## 2. Soroban Example Contracts Repository

The [stellar/soroban-examples](https://github.com/stellar/soroban-examples)
repository collects reference implementations that demonstrate Soroban
patterns. Submissions are made via pull request.

### What to submit

The `WatcherRegistry` contract is a clean, well-documented example of:

- Multi-admin authorization with `require_auth()`
- Idempotent registration patterns
- On-chain audit events with `env.events().publish()`
- Instance storage with `Vec<Address>`
- Error handling with `#[contracterror]`

The `AlertRegistry` contract additionally demonstrates:

- Persistent storage with TTL extension
- Dual-index lookups (owner index + contract index)
- Paginated query variants
- Per-owner rate limiting enforced on-chain

### Submission checklist

Before opening the PR, verify:

- [ ] Both contracts build cleanly:
  ```bash
  cargo build --release --target wasm32-unknown-unknown
  ```
- [ ] All tests pass:
  ```bash
  cargo test
  ```
- [ ] Clippy is clean:
  ```bash
  cargo clippy -- -D warnings
  ```
- [ ] Each contract has a top-level rustdoc comment explaining its purpose.
- [ ] Each public function has a rustdoc comment with `# Auth`, `# Arguments`,
      and `# Returns` sections.
- [ ] A `README.md` exists at the contract crate root (see template below).

### Per-contract README template

Create `contracts/watcher-registry/README.md` and
`contracts/alert-registry/README.md` using this template:

```markdown
# <ContractName>

> Part of [stellar-txwatch-contracts](https://github.com/Tx-wat/stellar-txwatch-contracts)

One-paragraph description of what the contract does and why it is useful as an
example.

## Key patterns demonstrated

- Pattern 1
- Pattern 2

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

## Function reference

See [docs/<contract-name>.md](../../docs/<contract-name>.md).
```

### How to submit

1. Fork [stellar/soroban-examples](https://github.com/stellar/soroban-examples).
2. Copy `contracts/watcher-registry/` into the fork as a top-level example
   crate (e.g. `watcher-registry/`).
3. Add the per-contract `README.md` described above.
4. Add the crate to the workspace `Cargo.toml` in the fork.
5. Open a pull request with the title:
   `feat: add watcher-registry example contract`
6. Reference the upstream repository and this submission guide in the PR body.

---

## 3. npm Package — TypeScript Bindings

TypeScript bindings for `WatcherRegistry` are published to npm as
`@tx-wat/watcher-registry` via the
[publish-bindings workflow](../.github/workflows/publish-bindings.yml).

### Publishing steps (manual)

If you need to publish manually outside of CI:

```bash
# 1. Build the WASM
cargo build --release --target wasm32-unknown-unknown

# 2. Generate bindings
stellar contract bindings typescript \
  --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
  --contract-id <WATCHER_REGISTRY_CONTRACT_ID> \
  --output-dir bindings/watcher-registry \
  --overwrite

# 3. Install deps and build
cd bindings/watcher-registry
npm install
npm run build

# 4. Publish
npm publish --access public
```

### Required secrets

| Secret | Description |
|---|---|
| `NPM_TOKEN` | npm automation token with publish rights to the `@tx-wat` scope |

Set this in **GitHub → Settings → Secrets and variables → Actions**.
