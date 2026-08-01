# Vendoring provenance

This is a modified vendor copy, not a pristine upstream archive.

| Field | Value |
|---|---|
| Upstream | `https://github.com/akinsella/yt-transcript-rs` |
| Upstream version/tag | `v0.1.8` / crate version `0.1.8` |
| Upstream commit | `126fc667d486c548d368dd1a9ba0f0459d46b0db` |
| Upstream Git tree | `08d16941b526a17e0227c6d547ac302960efd929` |
| Imported | 2026-08-01 |
| License | Upstream MIT license retained in `LICENSE` |

## Local deltas

- Removed Reqwest's `cookies` feature from the vendor manifest.
- Removed the public cookie-jar module and its re-export; the upstream
  `src/cookie_jar_loader.rs` is deliberately not shipped.
- Kept `YouTubeTranscriptApi::new` source-compatible, but a non-`None`
  cookie-file path now returns `CookieError::Invalid` explaining that cookie-file
  authentication is unavailable in this portable build.
- Updated the vendored README to remove cookie-file authentication claims and
  document the portable limitation.
- Added narrow `dead_code` allowances for two transcript-parser types made
  unreachable by the cookie-module removal, and stripped trailing whitespace
  from imported Rust/README files for repository formatting checks.

RZN Tools supplies explicit request cookies at its connector boundary when
needed. Browser-profile and cookie-file extraction remain outside this crate.
