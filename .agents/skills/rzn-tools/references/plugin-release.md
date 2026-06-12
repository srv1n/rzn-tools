# Plugin Release Reference

Use this before building, publishing, or claiming completion for the `rzn-tools` plugin bundle.

## Contents

- [Completion Rule](#completion-rule)
- [Repo Build Facts](#repo-build-facts)
- [Backend Contract](#backend-contract)
- [Live Catalog Checks](#live-catalog-checks)
- [Why This Matters](#why-this-matters)

## Completion Rule

A plugin release is complete only after all of this succeeds in order:

```text
build immutable ZIP
  -> register/publish to local backend
  -> verify local catalog/artifact
  -> register/publish to cloud backend
  -> verify cloud catalog/artifact
```

Local backend comes first:

```text
http://localhost:8082
```

Cloud backend comes second:

```text
https://cloud.rzn.ai
```

Stop on the first failure and report the exact failed step and environment. Do not describe a release as done if the ZIP exists but the backend catalog was not updated.

## Repo Build Facts

The plugin config is:

```text
scripts/plugins/config/rzn-tools.json
```

The MCP binary payload is:

```text
target/release/rzn-tools-mcp
```

The plugin build target expects full connector features:

```bash
cargo build --release -p rzn_tools_mcp --features full
```

The Makefile target for local ZIP building is:

```bash
make plugins-build-rzn-tools-macos-arm64
```

It requires a signing key at:

```text
.secrets/plugin-signing/ed25519.private
```

Generate dev keys only when the task is explicitly local/dev:

```bash
make plugins-keygen
```

Verify a bundle with:

```bash
make plugins-verify ZIP=dist/plugins/.../rzn-tools-...zip PUB=.secrets/plugin-signing/ed25519.public
```

## Backend Contract

Preferred scoped publisher flow:

1. `POST /publisher/products/:product_id/releases`
2. `POST /publisher/releases/:release_id/upload-session`
3. Upload ZIP to the returned presigned URL.
4. `POST /publisher/releases/:release_id/finalize`
5. `POST /publisher/releases/:release_id/publish`

Legacy admin flow remains compatible:

1. `POST /admin/plugins/releases`
2. `POST /admin/plugins/catalog/publish`

Use the repo's existing release/publish script if the user asks for the established automation, but do not introduce new Python scripts.

## Live Catalog Checks

Stable catalog:

```bash
curl -fsS http://localhost:8082/plugins/index.json
curl -fsS https://cloud.rzn.ai/plugins/index.json
```

Non-stable channels:

```bash
curl -fsS "http://localhost:8082/plugins/index.json?channel=nightly"
curl -fsS "https://cloud.rzn.ai/plugins/index.json?channel=nightly"
```

## Why This Matters

Uploading a ZIP alone does nothing for users. The backend serves only registered and published catalog entries. The signed catalog pointer is the live switch.
