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

## Cutting a Release

Releases follow a specific two-phase trigger sequence due to how GitHub Actions workflows are structured:

1. **Git Tag (`vX.Y.Z`)** triggers `.github/workflows/deploy-testnet.yml`, which compiles the contracts, deploys them to Stellar testnet, updates `DEPLOYMENTS.md`, and opens a pull request with the new addresses.
2. **GitHub Release (`published`)** triggers `.github/workflows/publish-abis.yml` and `.github/workflows/publish-bindings.yml`, which generate the JSON ABIs, upload them to the release assets, and generate/publish the TypeScript npm bindings.

Because these triggers are decoupled, releases must follow the sequence below in exact order.

### Release Sequence

1. **Prepare Release**
   - Ensure all target PRs are merged to `main`.
   - Update `CHANGELOG.md` by moving items from `[Unreleased]` to a new version header `[X.Y.Z] - YYYY-MM-DD`.
   - Commit and push changes to `main`.

2. **Create and Push Git Tag**
   ```bash
   git checkout main
   git pull origin main
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```

3. **Verify Testnet Deployment & Merge Addresses**
   - Monitor the **Deploy to Testnet** workflow on GitHub Actions.
   - Once completed, review and merge the automated PR (`deploy/update-deployments-vX.Y.Z`) into `main`.

4. **Publish GitHub Release**
   - In GitHub, navigate to **Releases** → **Draft a new release**.
   - Select the existing tag `vX.Y.Z`.
   - Set the title to `vX.Y.Z` and paste the release notes from `CHANGELOG.md`.
   - Click **Publish release**.

5. **Verify Automated Publishing**
   - Monitor the **Publish Contract ABIs** and **Publish TypeScript Bindings** workflows triggered by the published release.

### Release Verification Checklist

- [ ] `vX.Y.Z` tag created and pushed to remote repository.
- [ ] `Deploy to Testnet` workflow completed successfully.
- [ ] Automated `deploy/update-deployments-vX.Y.Z` PR reviewed and merged to `main`.
- [ ] GitHub Release `vX.Y.Z` created and published using the tag.
- [ ] `Publish Contract ABIs` workflow passed and attached ABI JSON files (`alert-registry.json`, `watcher-registry.json`) to the release.
- [ ] `Publish TypeScript Bindings` workflow passed and published the latest package to npm.

## Sister Repos
 
- **Core engine:** https://github.com/Tx-wat/stellar-txwatch-core
- **Web dashboard:** https://github.com/Tx-wat/stellar-txwatch-web

See [docs/compatibility.md](docs/compatibility.md) for the cross-repository compatibility matrix and release checklist.

