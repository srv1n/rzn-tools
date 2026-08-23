# Contributing

Read [Development](docs/system/development.md) before a change.

## Build rule

Use Make targets for all Rust compilation and tests. Do not run Cargo build, check, test, lint, doc, or run commands directly.

```bash
make fmt
make clippy CARGO_ARGS="--all-targets --all-features -- -D warnings"
make test CARGO_ARGS="--workspace"
RUSTDOCFLAGS="-D warnings" make doc CARGO_ARGS="--workspace"
```

## Code changes

- Use the existing `Connector` trait.
- Add one canonical connector name and one canonical tool name.
- Mock remote calls in tests.
- Do not use personal data in tests.
- State all write effects in tool descriptions.
- Update `docs/system/` when public behavior changes.

## Connector changes

1. Change the module under `rzn_tools_core/src/connectors/`.
2. Add or update its Cargo feature.
3. Update `build_registry_enabled_only`.
4. Update resolver rules when URL routing changes.
5. Add a focused test.
6. Update [Connectors](docs/system/connectors.md).

## Documentation

Use short sentences. Use one term for one thing. Write in the present tense.

```bash
tusker docs map --json
tusker validate --json
```

## Commits

Use a short imperative subject. Conventional Commit prefixes are preferred.
