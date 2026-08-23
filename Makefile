.PHONY: help ensure-sccache build build-release test check clippy fmt fmt-check doc run install release release-dry-run release-prepare release-retitle-legacy release-retitle-legacy-dry-run plugins-keygen plugins-build-rzn-tools-macos-arm64 plugins-verify plugins-validate-system-metadata

# Rust entrypoint policy
#
# Use these targets from every checkout. sccache's
# user-level cache is outside the repository, so worktrees share it.
SCCACHE ?= sccache
export RUSTC_WRAPPER := $(SCCACHE)
CARGO_ARGS ?=

help:
	@printf '%s\n' \
		'Rust commands (all require sccache):' \
		'  make build CARGO_ARGS="-p rzn_tools_cli --features full"' \
		'  make build-release CARGO_ARGS="-p rzn_tools_cli --features full"' \
		'  make test CARGO_ARGS="-p rzn_tools_core"' \
		'  make check | make clippy | make fmt | make fmt-check | make doc' \
		'  make run CARGO_ARGS="-p rzn_tools_cli -- list"'

ensure-sccache:
	@command -v "$(SCCACHE)" >/dev/null 2>&1 || { \
		echo "sccache is required. Install it with: cargo install sccache --locked" >&2; \
		exit 1; \
	}

build: ensure-sccache
	cargo build $(CARGO_ARGS)

build-release: ensure-sccache
	cargo build --release $(CARGO_ARGS)

test: ensure-sccache
	cargo test $(CARGO_ARGS)

check-server-profile: ensure-sccache
	cargo build -p rzn_tools_mcp --no-default-features --features server-full
	@for dep in rookie publicsuffix rusqlite; do \
		if cargo tree -p rzn_tools_mcp --no-default-features --features server-full -i $$dep >/dev/null 2>&1; then \
			echo "server-full unexpectedly includes $$dep"; exit 1; \
		fi; \
	done

check: ensure-sccache
	cargo check $(CARGO_ARGS)

clippy: ensure-sccache
	cargo clippy $(CARGO_ARGS)

fmt:
	cargo fmt --all $(CARGO_ARGS)

fmt-check:
	cargo fmt --all -- --check

doc: ensure-sccache
	cargo doc --no-deps $(CARGO_ARGS)

run: ensure-sccache
	cargo run $(CARGO_ARGS)

# -----------------------------------------------------------------------------
# RZN Desktop Extension Bundle (plugin.json + plugin.sig + payload ZIP)
#
# This repo ships rzn-tools as an MCP server (`rzn-tools-mcp`). To make it usable in the
# desktop app without compiling the desktop, we package it as a signed "extension"
# ZIP (aka plugin bundle) that rznapp can install from file or from a backend
# catalog.
#
# The packager/signing tool (`rzn_plugin_devkit`) currently lives in the sibling
# repo `../rzn-browser`. For now we call it via `--manifest-path` to keep
# `rznapp` clean and avoid copying the devkit into every repo.
# -----------------------------------------------------------------------------

RZN_PLUGIN_DEVKIT_MANIFEST ?= ../rzn-browser/Cargo.toml
RZN_TOOLS_PLUGIN_CONFIG ?= scripts/plugins/config/rzn-tools.json
RELEASE_REMOTE ?= origin
RELEASE_BRANCH ?= main

release:
	@python3 scripts/release.py \
		$(if $(VERSION),--version $(VERSION),) \
		--remote "$(RELEASE_REMOTE)" \
		--branch "$(RELEASE_BRANCH)" \
		$(if $(filter 1 true yes,$(ALLOW_DIRTY)),--allow-dirty,) \
		$(if $(filter 1 true yes,$(ALLOW_NON_MAIN)),--allow-non-main,) \
		$(if $(filter 1 true yes,$(SKIP_REMOTE_CHECK)),--skip-remote-check,)

release-dry-run:
	@python3 scripts/release.py \
		$(if $(VERSION),--version $(VERSION),) \
		--remote "$(RELEASE_REMOTE)" \
		--branch "$(RELEASE_BRANCH)" \
		--dry-run \
		$(if $(filter 1 true yes,$(ALLOW_DIRTY)),--allow-dirty,) \
		$(if $(filter 1 true yes,$(ALLOW_NON_MAIN)),--allow-non-main,) \
		$(if $(filter 1 true yes,$(SKIP_REMOTE_CHECK)),--skip-remote-check,)

release-prepare:
	@mkdir -p target/release-preflight
	@python3 scripts/release.py \
		$(if $(VERSION),--version $(VERSION),) \
		--remote "$(RELEASE_REMOTE)" \
		--branch "$(RELEASE_BRANCH)" \
		--dry-run \
		$(if $(filter 1 true yes,$(ALLOW_DIRTY)),--allow-dirty,) \
		$(if $(filter 1 true yes,$(ALLOW_NON_MAIN)),--allow-non-main,) \
		$(if $(filter 1 true yes,$(SKIP_REMOTE_CHECK)),--skip-remote-check,)
	@python3 scripts/generate_release_notes.py \
		--tag "v$(or $(VERSION),$(shell cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="rzn_tools_cli") | .version'))" \
		--allow-unreleased-tag \
		--output target/release-preflight/release-notes.md
	@echo "Preflight notes: target/release-preflight/release-notes.md"

release-retitle-legacy:
	@python3 scripts/retitle_github_releases.py --apply

release-retitle-legacy-dry-run:
	@python3 scripts/retitle_github_releases.py

# Build and install the local CLI binary.
# Usage: make install [INSTALL_DIR=/usr/local/bin] [FEATURES=full]
install:
	./packaging/scripts/local-install.sh

# Generate dev plugin signing keys used for the "install-from-file" loop.
plugins-keygen:
	@echo "🔑 Generating plugin signing keypair (.secrets/plugin-signing)..."
	cargo run --manifest-path "$(RZN_PLUGIN_DEVKIT_MANIFEST)" -p rzn_plugin_devkit -- \
		keygen --out .secrets/plugin-signing

# Validate bundled system metadata + starter quickstarts before packaging.
plugins-validate-system-metadata:
	@echo "🧪 Validating bundled system metadata..."
	cargo test -p rzn_tools_core --test system_metadata_conformance

# Build a signed rzn-tools extension ZIP (macos_arm64).
#
# Output:
#   dist/plugins/rzn-tools/<version>/macos_arm64/rzn-tools-<version>-macos_arm64.zip
plugins-build-rzn-tools-macos-arm64:
	@echo "📦 Building rzn-tools plugin ZIP (macos_arm64)..."
	@if [ ! -f ".secrets/plugin-signing/ed25519.private" ]; then \
		echo "[ERROR] Missing signing key. Run: make plugins-keygen"; \
		exit 1; \
	fi
	@$(MAKE) plugins-validate-system-metadata
	@# rzn-tools MCP server binary must include connector features (default is empty).
	@cargo build --release -p rzn_tools_mcp --features full
	@RZN_TOOLS_MCP_BIN_MACOS_ARM64="$(PWD)/target/release/rzn-tools-mcp" \
	cargo run --manifest-path "$(RZN_PLUGIN_DEVKIT_MANIFEST)" -p rzn_plugin_devkit -- \
		build \
		--config "$(RZN_TOOLS_PLUGIN_CONFIG)" \
		--platform macos_arm64 \
		--key .secrets/plugin-signing/ed25519.private \
		--out dist/plugins

# Verify a bundle ZIP (signature + sha256 payload map).
plugins-verify:
	@if [ -z "$(ZIP)" ] || [ -z "$(PUB)" ]; then \
		echo "Usage: make plugins-verify ZIP=dist/plugins/.../rzn-tools-...zip PUB=.secrets/plugin-signing/ed25519.public"; \
		exit 1; \
	fi
	cargo run --manifest-path "$(RZN_PLUGIN_DEVKIT_MANIFEST)" -p rzn_plugin_devkit -- \
		verify --zip "$(ZIP)" --public "$(PUB)"
