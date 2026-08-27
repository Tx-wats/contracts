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
5. Update the relevant doc in `docs/`.

## Sister Repos
 
- **Core engine:** https://github.com/Tx-wat/stellar-txwatch-core
- **Web dashboard:** https://github.com/Tx-wat/stellar-txwatch-web

See [docs/compatibility.md](docs/compatibility.md) for the cross-repository compatibility matrix and release checklist.

