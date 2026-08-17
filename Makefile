.PHONY: build test lint fmt deploy-testnet bindings clean

build:
	cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry

test:
	cargo test --workspace --locked

lint:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all

deploy-testnet:
	bash scripts/deploy.sh

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
