.PHONY: build test lint fmt check-events shellcheck deploy-testnet verify upgrade audit \
        bindings bindings-watcher bindings-alert bindings-all clean spec-check spec-golden-update \
        fuzz-build proptest-nightly e2e

build:
	cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry

test:
	cargo test --workspace --locked

# Mirrors the CI clippy steps: default features first, then --all-features so
# code behind `watcher-gating` and `testutils` is linted too.
lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

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
	bash scripts/generate-watcher-bindings.sh \
		target/wasm32-unknown-unknown/release/watcher_registry.wasm \
		"$(WATCHER_CONTRACT_ID)"
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

# Contract spec golden-file check.
# Fails if the committed contracts/*/spec.golden.json no longer matches the
# spec embedded in the built (and optimized) WASM.
spec-check:
	bash scripts/spec-golden.sh

# Overwrite the committed golden files with the spec extracted from the built
# WASM. Run only after building and optimizing the release WASM.
spec-golden-update:
	bash scripts/update-spec-golden.sh

# Mirror of .github/workflows/fuzz.yml (build step). Requires nightly + cargo-fuzz.
fuzz-build:
	cd contracts/alert-registry && cargo +nightly fuzz build validate_rule && cargo +nightly fuzz build register_alert

# Mirror of .github/workflows/nightly-proptest.yml.
proptest-nightly: build
	cargo test --workspace --locked -- --no-fail-fast

# Mirror of .github/workflows/deploy-e2e.yml: start stellar/quickstart in local
# mode via docker, optimize the WASM, then run deploy.sh against it.
# Requires: docker, stellar CLI. Stop with: docker rm -f quickstart-local
E2E_RPC_URL := http://localhost:8000/soroban-rpc
E2E_PASSPHRASE := Standalone Network ; February 2017

e2e: build
	docker rm -f quickstart-local 2>/dev/null || true
	docker run -d -p 8000:8000 --name quickstart-local stellar/quickstart --local --core-log-level error
	for i in $$(seq 1 80); do \
		curl -sf http://localhost:8000/ >/dev/null && curl -s $(E2E_RPC_URL) >/dev/null && break; \
		sleep 2; \
	done
	stellar contract optimize --wasm target/wasm32-unknown-unknown/release/alert_registry.wasm
	stellar contract optimize --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm
	stellar network add local --rpc-url $(E2E_RPC_URL) --network-passphrase "$(E2E_PASSPHRASE)"
	stellar keys generate deployer --network local --overwrite
	curl -s "http://localhost:8000/friendbot?addr=$$(stellar keys address deployer)" >/dev/null
	NETWORK=local RPC_URL=$(E2E_RPC_URL) NETWORK_PASSPHRASE="$(E2E_PASSPHRASE)" \
		FRIENDBOT_URL=http://localhost:8000/friendbot STELLAR_IDENTITY=deployer IDENTITY=deployer \
		bash scripts/deploy.sh
