# Vendored Dependencies

## rusty_ytdl

This directory contains a patched version of the `rusty_ytdl` crate.
It is vendored to include fixes or modifications required for RZN-Tools that have not been upstreamed yet or to ensure stability against upstream changes.

See `Cargo.toml` in the workspace root for the patch configuration:

```toml
[patch.crates-io]
rusty_ytdl = { path = "vendor/rusty_ytdl" }
```
