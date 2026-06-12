# Repository Guidelines

## Project Structure & Module Organization
- `rzn_tools_core/` contains the core library, connector trait, registry, and shared utilities.
- `rzn_tools_cli/` is the CLI binary; `rzn_tools_mcp/` is the MCP server binary.
- `scrapable_derive/` holds the proc-macro for HTML parsing.
- `docs/` houses connector and architecture docs; `packaging/` includes install scripts/formulas.
- `vendor/` contains vendored dependencies; config lives in `Cargo.toml`, `.cargo/config.toml`, `clippy.toml`, and `rustfmt.toml`.

## Build, Test, and Development Commands
- `cargo build` / `cargo build --release -p rzn_tools_cli` for debug/release builds.
- Feature-scoped builds: `cargo build --release -p rzn_tools_cli --features "youtube,hackernews"`.
- Release builds: ALWAYS build with all features enabled: `cargo build --release -p rzn_tools_cli --features full`.
- Run CLI: `cargo run -p rzn_tools_cli -- list`.
- Run MCP server: `cargo run -p rzn_tools_mcp`.
- Lint and format: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`.
- Full test suite: `cargo test --workspace`.

## Coding Style & Naming Conventions
- Rust formatting is enforced by `rustfmt` with 100-char line width and grouped imports (std/external/crate).
- Clippy uses pedantic lints; warnings are treated as errors.
- Connector modules live under `rzn_tools_core/src/connectors/` and should follow existing connector naming patterns.

## Testing Guidelines
- Add unit tests for new logic; integration tests where applicable.
- Mock external API calls in tests; avoid real network calls in CI.
- Ensure docs build cleanly: `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`.

## Commit & Pull Request Guidelines
- Recent history uses short, imperative summaries (e.g., “Add …”, “Fix …”) and release messages (“Release v0.2.7”).
- CONTRIBUTING.md requests Conventional Commits (e.g., `feat:`, `fix:`, `docs:`); prefer this format for new work.
- PRs should include a clear description, updated docs if needed, and pass fmt/clippy/tests/docs.
- Update `CHANGELOG.md` for user-facing changes.

## Security & Configuration Tips
- Use `.env.example` as the template; avoid committing secrets. Dependency checks use `cargo audit` and `cargo deny check`.

## Agent-Specific Instructions
- Do not access personal-data connectors (mail, notes, messages, reminders, contacts) without explicit user permission.
- When testing such connectors, provide commands for the user to run and wait for their feedback.

## Plugin Release Requirement

If the task includes building or publishing the `rzn-tools` plugin bundle, release
completion also requires backend notification using the contract documented at:

- `/Users/sarav/Downloads/side/rzn/backend/docs/runbook/plugin_team_release_guide.md`

For plugin release work:

- Building a ZIP alone is not enough.
- Notify the backend through the release registration and catalog publish API flow.
- Publish to local `http://localhost:8082` first, then cloud `https://cloud.rzn.ai`, unless the user explicitly says otherwise.
- The release script supports `cloud` directly and retains `prod` as a legacy alias.
- If local or cloud publish fails at any stage, stop and report exactly what failed.
