.DEFAULT_GOAL := test
.PHONY: build test conformance package example
CARGO ?= cargo
JELTO_CONTRACTS_DIR ?=
JELTO_CONTRACTS_VERSION = $(shell python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$(JELTO_CONTRACTS_DIR)/spec/contracts/manifest.json")
CONFORMANCE_HOST = $(CURDIR)/target/debug/conformance-host$(if $(filter Windows_NT,$(OS)),.exe,)

node_modules: package.json package-lock.json
	npm ci
	@touch $@

build: node_modules
	npm run build
	$(CARGO) build --locked --no-default-features --features conformance

test: node_modules
	$(CARGO) test --locked --features conformance -- --nocapture --test-threads=1
	npm test
	npm run build

conformance: build
	@test -n "$(JELTO_CONTRACTS_DIR)" || { echo 'Set JELTO_CONTRACTS_DIR to a verified Jelto contracts archive.' >&2; exit 1; }
	go -C "$(JELTO_CONTRACTS_DIR)" run ./spec/conformance/runner -contracts-version "$(JELTO_CONTRACTS_VERSION)" -host "$(CONFORMANCE_HOST)"

package: node_modules
	CARGO="$(CARGO)" python3 package.py

example: node_modules
	npm pack --pack-destination .
	node scripts/prepare-example.mjs
	cd example && npm ci
	cd example && npm run tauri build -- --no-bundle
