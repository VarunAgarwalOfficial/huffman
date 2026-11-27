.PHONY: dev build check lint

# Serves the demo page at http://localhost:8000.
dev:
	python3 -m http.server 8000 --bind 127.0.0.1 --directory web

# Rebuilds the library and the page's WebAssembly module.
build:
	cargo build --release --workspace
	cargo build --release -p huffman-wasm --target wasm32-unknown-unknown
	cp target/wasm32-unknown-unknown/release/huffman_wasm.wasm web/huffman.wasm

check:
	cargo build --workspace
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	npx --yes @biomejs/biome@2.6.7 check
