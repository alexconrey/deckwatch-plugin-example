WASM_TARGET := wasm32-unknown-unknown
HOST := $(shell rustc -vV | sed -n 's/host: //p')
OUT := target/$(WASM_TARGET)/release/deckwatch_plugin_example.wasm
DIST := dist/plugin.wasm

.PHONY: build test check clean

build:
	cargo build --locked --release --target $(WASM_TARGET)
	mkdir -p dist
	cp $(OUT) $(DIST)
	@echo "artifact: $(DIST) ($$(wc -c < $(DIST)) bytes)"

# Unit tests run on the host: .cargo/config.toml makes wasm32 the default target.
test:
	cargo test --locked --target $(HOST)

# What CI runs.
check:
	cargo fmt --check
	cargo clippy --locked --target $(HOST) --all-targets -- -D warnings
	cargo clippy --locked --target $(WASM_TARGET) -- -D warnings
	cargo test --locked --target $(HOST)

clean:
	cargo clean
	rm -f $(DIST) dist/SHA256SUMS
