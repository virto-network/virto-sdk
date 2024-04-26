build-web:
	@mkdir -p dist
	cargo build --release --target wasm32-unknown-unknown
	@cp ./target/wasm32-unknown-unknown/release/pjs.wasm dist/
	wasm-bindgen --out-dir dist --target web --no-typescript --remove-name-section dist/pjs.wasm

test:
	cargo test
