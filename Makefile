.PHONY: build test lint fmt check-events shellcheck deploy-testnet verify upgrade audit \
	bindings bindings-watcher bindings-alert bindings-all clean

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
	shellcheck -x scripts/*.sh scripts/lib/*.sh

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

# Generate TypeScript bindings. Requires: stellar CLI on PATH.
# Usage: CONTRACT_ID=CXXX... make bindings-watcher
#        [ALERT_CONTRACT_ID=CXXX...] make bindings-alert
#        WATCHER_CONTRACT_ID=CXXX... ALERT_CONTRACT_ID=CYYY... make bindings-all
# `make bindings` is kept as an alias for bindings-watcher.
WATCHER_CONTRACT_ID ?= $(CONTRACT_ID)

bindings: bindings-watcher

bindings-watcher: build
	stellar contract bindings typescript \
		--wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
		--contract-id $(WATCHER_CONTRACT_ID) \
		--output-dir bindings/watcher-registry \
		--overwrite
	cd bindings/watcher-registry && npm install && npm run build

# The alert package generates into its own dist/ (see its package.json), so
# only override the contract ID when one is given; otherwise use its pinned ID.
bindings-alert: build
	cd bindings/alert-registry && npm install && \
		$(if $(ALERT_CONTRACT_ID),stellar contract bindings typescript \
			--wasm ../../target/wasm32-unknown-unknown/release/alert_registry.wasm \
			--contract-id $(ALERT_CONTRACT_ID) \
			--output-dir ./dist \
			--overwrite,npm run build)

bindings-all: bindings-watcher bindings-alert

clean:
	cargo clean
	rm -rf bindings/watcher-registry/dist bindings/watcher-registry/node_modules
	rm -rf bindings/alert-registry/dist bindings/alert-registry/node_modules
