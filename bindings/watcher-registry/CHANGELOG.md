# Changelog — `@tx-wat/watcher-registry`

All notable changes to the `@tx-wat/watcher-registry` TypeScript package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Client method and type definitions for `getWatcherCount()` returning total registered watchers count.
- Support for `clearAllWatchers({ admin })` to deauthorize all watchers in a single transaction.
- Support for `replaceWatcher({ admin, old_watcher, new_watcher })` atomic rotation.
- Dual ESM (`dist/index.mjs`) and CommonJS (`dist/index.js`) bundle exports.
- Vitest test harness for verifying contract binding signatures.

## [0.1.0] - 2025-05-28

### Added
- Initial release of TypeScript bindings for the `WatcherRegistry` Soroban contract.
- Multi-admin management methods: `initialize`, `addAdmin`, `removeAdmin`, `transferAdmin`, `getAdmins`, `getAdmin`.
- Watcher authorization methods: `registerWatcher`, `removeWatcher`, `isWatcherAuthorized`, `isAuthorized`, `getWatchers`.
- Complete TypeScript type declarations and error enum bindings for `ContractError`.
- Peer dependency support for `@stellar/stellar-sdk` (>=12.0.0).

[Unreleased]: https://github.com/Tx-wat/stellar-txwatch-contracts/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Tx-wat/stellar-txwatch-contracts/releases/tag/v0.1.0
