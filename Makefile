.DEFAULT_GOAL := test
.PHONY: build test conformance conformance-run conformance-twice package example security check npm-audit cargo-audit lint deadcode machete

# The shared quality targets (hooks, tools, secrets, osv, workflow-audit, sast)
# arrive from the contracts repository as a checksummed snapshot; `make inputs`
# in the backend refreshes it, and nothing edits it here.
include vendor/quality/quality.mk
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
	$(MAKE) conformance-run

# The runner invocation alone, for a tree `build` has already produced.
conformance-run:
	@test -n "$(JELTO_CONTRACTS_DIR)" || { echo 'Set JELTO_CONTRACTS_DIR to a verified Jelto contracts archive.' >&2; exit 1; }
	go -C "$(JELTO_CONTRACTS_DIR)" run ./spec/conformance/runner -contracts-version "$(JELTO_CONTRACTS_VERSION)" -host "$(CONFORMANCE_HOST)"

# spec/sdk-conformance.md §1: every scenario passes on a clean machine, twice.
# The passes share nothing -- each runner starts its own mockd on port 0 with a
# private control socket and gives every scenario a fresh JELTO_STATE_DIR under
# its own temporary directory -- so they run concurrently and the gate takes one
# pass's wall time, not two. The first pass's output is replayed once the second
# has finished so the log reads as two complete passes.
conformance-twice: build
	$(MAKE) conformance-run > "$${TMPDIR:-/tmp}/jelto-conformance-$$$$.log" 2>&1 & pid=$$!; \
	$(MAKE) conformance-run; second=$$?; \
	wait $$pid; first=$$?; \
	echo; echo '--- first pass (ran concurrently with the one above) ---'; \
	cat "$${TMPDIR:-/tmp}/jelto-conformance-$$$$.log"; rm -f "$${TMPDIR:-/tmp}/jelto-conformance-$$$$.log"; \
	test "$$first" -eq 0 && test "$$second" -eq 0

package: node_modules
	CARGO="$(CARGO)" python3 package.py

example: node_modules
	npm pack --pack-destination .
	node scripts/prepare-example.mjs
	cd example && npm ci
	cd example && npm run tauri build -- --no-bundle

# The whole gate: what to run before a pull request, and what the Linux row of
# CI runs. The macOS and Windows rows repeat the platform-sensitive part (test
# build package); every row also builds the native example, and conformance
# twice runs on release or on request.
check: lint deadcode test build package security

# rustfmt, then Clippy over every target of the crate with warnings as errors.
# The lint levels are Cargo.toml's [lints.clippy]; clippy.toml holds settings.
lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --locked --all-targets --features conformance -- -D warnings

deadcode: machete

# Cargo dependencies that no source file names, for the crate and the native
# example: cargo-machete walks every Cargo.toml below the root that Git does
# not ignore, and --skip-target-dir keeps packaged copies in target/ out.
machete:
	$(QUALITY_TOOL) run cargo-machete --skip-target-dir .

security: secrets workflow-audit osv npm-audit cargo-audit

npm-audit:
	npm audit --audit-level=moderate --package-lock-only

# RustSec's own scanner, beside OSV-Scanner. CI installs the pinned
# cargo-audit; locally, `cargo install cargo-audit --locked` provides it.
cargo-audit:
	$(CARGO) audit
