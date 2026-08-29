# Contributing

## Prerequisites

Install the Rust toolchain and Soroban target:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```

Install the Stellar CLI:

```bash
cargo install --locked stellar-cli --features opt
```

## Minimum Supported Rust Version (MSRV)

The workspace minimum supported Rust version (MSRV) is **1.88**, as declared in the root `Cargo.toml` (`rust-version = "1.88"`).

- **CI Enforcement:** Verified continuously on every pull request and push to `main` via the `msrv` job in `.github/workflows/ci.yml` running `cargo check --workspace`.
- **Bump Triggers:** MSRV is only bumped when strictly required by a necessary dependency update (e.g., newer `soroban-sdk` releases) or essential compiler features.
- **Policy & Cadence:** MSRV bumps are not made casually. Any increase is considered a breaking change, documented in `CHANGELOG.md`, and accompanied by a corresponding update to `rust-version` in `Cargo.toml` and CI configuration.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

Tests run natively (no WASM target needed). Each contract has a `#[cfg(test)]` module covering happy paths, unauthorized rejections, and edge cases.

## Deploy to Testnet

1. Set up a funded testnet identity:

```bash
stellar keys generate deployer --network testnet
stellar keys fund deployer --network testnet
```

2. Run the deploy script:

```bash
bash scripts/deploy.sh
```

3. Update `DEPLOYMENTS.md` with the printed contract addresses.

## Adding a New Function to an Existing Contract

1. Add the function inside the `#[contractimpl]` block in `contracts/<name>/src/lib.rs`.
2. If it mutates state, call `<caller>.require_auth()` as the first line.
3. Add at least one test in the `#[cfg(test)]` module covering the happy path and any auth rejection.
4. Run `cargo test` to confirm everything passes.
5. Update the relevant documentation to prevent documentation drift:
   - If this emits a new event, update `docs/events.md`.
   - If this adds a storage key, update `docs/storage.md`.
   - If this changes TTL behavior, update `docs/ttl.md`.
   - Update contract-specific documentation in `docs/<contract-name>.md`.
6. Note: An automated doc-sync check (issue #110) runs in CI as a backstop to catch cases where these manual documentation steps are missed.

## Adding a New Contract

When scaffolding an entirely new contract crate in this repository, follow these steps (see `contracts/alert-registry` as a complete worked reference):

### 1. Create Crate Structure & Workspace Registration
Create a directory under `contracts/<contract-name>` with `src/lib.rs`, `Cargo.toml`, and `build.rs`. Register the new crate in the root `Cargo.toml` under `[workspace] members`:

```toml
# Cargo.toml
[workspace]
members = [
    "contracts/alert-registry",
    "contracts/watcher-registry",
    "contracts/<contract-name>",
    "contracts/integration-tests",
    "contracts/test-utils",
]
```

### 2. Configure Contract `Cargo.toml`
Set up `contracts/<contract-name>/Cargo.toml` with the appropriate crate types and workspace dependencies:

```toml
[package]
name = "<contract-name>"
version = "0.1.0"
edition = "2021"
rust-version.workspace = true

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
soroban-sdk = { workspace = true }

[dev-dependencies]
soroban-sdk = { workspace = true, features = ["testutils"] }
```

### 3. Add `build.rs`
Copy or create `build.rs` in the contract crate root to ensure compatibility across target OS/environments (e.g., Windows/GNU export-table limit):

```rust
// contracts/<contract-name>/build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
    {
        println!("cargo:rustc-link-arg=-Wl,--exclude-all-symbols");
    }
}
```

### 4. Scaffold TypeScript Bindings Package
1. Create `bindings/<contract-name>/` with `package.json`, `tsconfig.json`, and `.npmignore` (refer to `bindings/alert-registry/`).
2. Generate bindings using the Stellar CLI:
   ```bash
   stellar contract bindings typescript \
     --wasm target/wasm32-unknown-unknown/release/<contract_name>.wasm \
     --contract-id <CONTRACT_ID> \
     --output-dir bindings/<contract-name> \
     --overwrite
   ```

### 5. Update CI Workflows and Deploy Scripts
1. **CI Build & Checks:** Add `-p <contract-name>` to the WASM build steps in:
   - `.github/workflows/ci.yml` (`Build (WASM)` step)
   - `.github/workflows/wasm-size-check.yml`
   - `.github/workflows/publish-bindings.yml` (and add bindings generation step)
   - `.github/workflows/publish-abis.yml`
2. **Deployment Script:** Update `scripts/deploy.sh` to include the contract in the optimization loop (`stellar contract optimize`) and add deployment/initialization commands for testnet and mainnet.
3. **Documentation:** Add a dedicated contract reference under `docs/<contract-name>.md`.

## Sister Repos
 
- **Core engine:** https://github.com/Tx-wat/stellar-txwatch-core
- **Web dashboard:** https://github.com/Tx-wat/stellar-txwatch-web

See [docs/compatibility.md](docs/compatibility.md) for the cross-repository compatibility matrix and release checklist.

