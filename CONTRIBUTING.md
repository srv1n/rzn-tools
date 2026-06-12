# Contributing to rzn-tools

Thank you for your interest in contributing to rzn-tools. This document provides guidelines and information for contributors.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## Getting Started

### Prerequisites

- Rust 1.75+ (MSRV) - install via [rustup.rs](https://rustup.rs)
- Git

### Optional Tools

```bash
# Security audit
cargo install cargo-audit

# Dependency license/ban checking
cargo install cargo-deny
```

### Setup

```bash
# Clone the repository
git clone https://github.com/srv1n/rzn-tools.git
cd rzn-tools

# Build the project
cargo build

# Run tests
cargo test

# Run the CLI
cargo run -p rzn_tools_cli -- list
```

## Development Workflow

### Before Submitting Code

Run all checks locally before pushing:

```bash
# Format code
cargo fmt --all

# Run linter (must pass with no warnings)
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test --workspace

# Check documentation builds
cargo doc --no-deps --workspace

# (Optional) Security audit
cargo audit

# (Optional) License/dependency check
cargo deny check
```

### Quick Check Alias

The project includes a cargo alias for linting:

```bash
cargo lint  # Equivalent to clippy with strict settings
```

## How to Contribute

### Reporting Bugs

1. Check existing [issues](https://github.com/srv1n/rzn-tools/issues) to avoid duplicates
2. Use the bug report template
3. Include:
   - Clear description of the issue
   - Steps to reproduce
   - Expected vs actual behavior
   - Environment details (OS, Rust version)

### Suggesting Features

1. Check existing issues and discussions
2. Use the feature request template
3. Describe the use case and proposed solution

### Submitting Code

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Run all checks (see above)
5. Commit with clear messages
6. Push and create a Pull Request

## Code Standards

### Formatting

We use `rustfmt` with custom settings in `rustfmt.toml`:
- Max line width: 100 characters
- Imports grouped by: std, external, crate

```bash
cargo fmt --all
```

### Linting

We use `clippy` with pedantic lints. The CI will fail on any warnings.

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Configuration is in `clippy.toml` and `Cargo.toml` under `[workspace.lints.clippy]`.

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/) style:

```
feat: add YouTube transcript chapter grouping
fix: handle rate limiting in Slack connector
docs: update installation instructions
refactor: simplify authentication flow
test: add integration tests for GitHub connector
deps: update reqwest to 0.12
ci: add MSRV check to workflow
```

### Testing

- Write unit tests for new functionality
- Add integration tests where applicable
- Mock external API calls in tests
- Ensure all tests pass before submitting PR

```bash
cargo test --workspace
```

### Documentation

- Update README.md if adding new features
- Add doc comments to public functions
- Update connector documentation in `docs/CONNECTORS.md`
- Include examples for new functionality

Doc comments should build without warnings:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## Adding New Connectors

### Connector Structure

1. Create a new module in `rzn_tools_core/src/connectors/`
2. Implement the `Connector` trait
3. Define tools using JSON Schema
4. Add authentication support if needed
5. Register in the provider registry
6. Add feature flag in `Cargo.toml`

### Connector Checklist

- [ ] Implement all required `Connector` trait methods
- [ ] Define clear tool descriptions and schemas
- [ ] Handle errors gracefully (no panics, no `unwrap()`)
- [ ] Add rate limiting/backoff for external APIs
- [ ] Return structured JSON responses
- [ ] Document authentication requirements
- [ ] Add feature flag in `rzn_tools_core/Cargo.toml`
- [ ] Forward feature in `rzn_tools_cli/Cargo.toml` and `rzn_tools_mcp/Cargo.toml`
- [ ] Update `docs/CONNECTORS.md`
- [ ] Add tests
- [ ] Run `cargo deny check` to verify dependencies

### Example Connector

```rust
use async_trait::async_trait;
use crate::{Connector, ConnectorError};

pub struct MyConnector {
    client: reqwest::Client,
}

#[async_trait]
impl Connector for MyConnector {
    fn name(&self) -> &'static str { "my-connector" }

    fn description(&self) -> &'static str {
        "Description of what this connector does"
    }

    // Implement other required methods...
}
```

## Pull Request Process

1. Ensure all CI checks pass
2. Update documentation as needed
3. Request review from maintainers
4. Address feedback promptly
5. Squash commits if requested

### PR Checklist

- [ ] CI passes (fmt, clippy, test, docs)
- [ ] No new clippy warnings
- [ ] Code is formatted (`cargo fmt`)
- [ ] Documentation updated
- [ ] Commit messages follow conventions
- [ ] PR description explains changes
- [ ] CHANGELOG.md updated (for user-facing changes)

## CI/CD

The project uses GitHub Actions for continuous integration:

| Check | Description |
|-------|-------------|
| `fmt` | Code formatting with rustfmt |
| `clippy` | Lint check with warnings as errors |
| `test` | Tests on Linux, macOS, Windows |
| `test-minimal` | Tests with no default features |
| `msrv` | Build with minimum supported Rust version |
| `docs` | Documentation builds without warnings |
| `security` | cargo-audit vulnerability scan |
| `deny` | cargo-deny license and dependency check |

All checks must pass before merging.

## Release Process

Releases are managed by maintainers:

1. Update version in `Cargo.toml` files
2. Update `CHANGELOG.md`
3. Create and push version tag: `git tag v0.x.0`
4. GitHub Actions builds and publishes releases after `make release`

## Project Structure

```
rzn-tools/
├── rzn_tools_core/       # Core library with Connector trait
├── rzn_tools_cli/        # CLI binary
├── rzn_tools_mcp/        # MCP server binary
├── scrapable_derive/ # Proc-macro for HTML parsing
├── vendor/           # Vendored dependencies
├── docs/             # Documentation
└── packaging/        # Installation scripts, Homebrew formula
```

## Getting Help

- [GitHub Issues](https://github.com/srv1n/rzn-tools/issues) - Bug reports and features
- [GitHub Discussions](https://github.com/srv1n/rzn-tools/discussions) - Questions and ideas

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (AGPL-3.0-only).

---

Thank you for contributing to rzn-tools.
