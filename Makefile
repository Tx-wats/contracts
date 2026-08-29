.PHONY: build test lint fmt check-events deploy-testnet bindings clean shellcheck
.PHONY: build test lint fmt check-events deploy-testnet verify upgrade audit bindings clean

build:
	cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry

test:
	cargo test --workspace --locked

lint:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all

check-events:
	bash scripts/check-events-doc.sh

shellcheck:
	shellcheck scripts/*.sh

deploy-testnet:
	bash scripts/deploy.sh

# Verify a deployed contract against local build.
# Usage: CONTRACT=alert-registry CONTRACT_ID=CXXX... [NETWORK=testnet] make verify
verify:
	bash scripts/verify.sh $(if $(CONTRACT),--contract $(CONTRACT)) $(if $(CONTRACT_ID),--contract-id $(CONTRACT_ID)) $(if $(NETWORK),--network $(NETWORK))

# Upgrade a deployed contract on-chain.
# Usage: CONTRACT=alert-registry CONTRACT_ID=CXXX... [NETWORK=testnet] make upgrade
upgrade:
	bash scripts/upgrade.sh $(if $(CONTRACT),--contract $(CONTRACT)) $(if $(CONTRACT_ID),--contract-id $(CONTRACT_ID)) $(if $(NETWORK),--network $(NETWORK))

# Run cargo audit dependency vulnerability scan.
audit:
	cargo audit

# Generate TypeScript bindings for WatcherRegistry.
# Requires: stellar CLI on PATH and a prior `make build`.
# Usage: CONTRACT_ID=CXXX... make bindings
bindings: build
	stellar contract bindings typescript \
		--wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
		--contract-id $(CONTRACT_ID) \
		--output-dir bindings/watcher-registry \
		--overwrite
	cd bindings/watcher-registry && npm install && npm run build

clean:
	cargo clean
	rm -rf bindings/watcher-registry/dist bindings/watcher-registry/node_modules
