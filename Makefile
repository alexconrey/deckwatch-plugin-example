WASM_TARGET := wasm32-unknown-unknown
OUT := target/$(WASM_TARGET)/release/deckwatch_plugin_example.wasm
DIST := dist/plugin.wasm

.PHONY: build test clean

build:
	cargo build --release --target $(WASM_TARGET)
	mkdir -p dist
	cp $(OUT) $(DIST)
	@echo "artifact: $(DIST) ($$(wc -c < $(DIST)) bytes)"

# Run unit tests on the host target (not WASM) so they can use std I/O.
test:
	cargo test --target $$(rustc -vV | sed -n 's/host: //p')

clean:
	cargo clean
	rm -f $(DIST)
