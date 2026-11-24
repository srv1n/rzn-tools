# Every connector can go through a proxy

Status: ready to build, 24 Nov 2026. Owner: rzn-tools. Backend decision:
`backend/docs/system/decisions/connections-can-go-through-a-proxy-profile.md`.

## Problem

Sites block cloud addresses. On 23 Nov YouTube answered every player request from a Hetzner box with "Sign in
to confirm you're not a bot", and the same request from a home network worked. Reddit does the same.

Only Reddit and Hacker News can use a proxy today. Each has its own private `proxy_url` handling
(`connectors/reddit/mod.rs` `reqwest_client`, `connectors/hackernews/mod.rs` `client_from_proxy`). The other
connectors build their own clients: 57 `Client::builder()`/`Client::new()` calls in 41 files, five through
`utils::build_reqwest_client`. YouTube alone builds three different clients.

## What to build

1. **One client constructor.** Extend `utils::build_reqwest_client` (or move it to `http.rs`) so it takes the
   connector's auth details and applies `proxy_url` with `reqwest::Proxy::all`. Every network connector builds
   every client through it, at the point it builds clients today. Reddit's and Hacker News's private copies
   are deleted.
2. **`proxy_url` is a common auth field.** It is declared once and merged into every network connector's
   auth fields, not repeated per connector. Its value may hold a user and password: never log it or echo it in
   an error. Error messages name the scheme and host only.
3. **SOCKS.** Turn on reqwest's `socks` feature so `socks5://` and `socks5h://` work. The first use is an SSH
   reverse tunnel (`ssh -R 127.0.0.1:1080 build-box`) from a home machine.
4. **Subprocesses.** yt-dlp gets `--proxy <url>`. Any other subprocess that fetches does the same.
5. **No direct fallback.** If `proxy_url` is set and the proxy cannot be reached, the call fails. The
   `no_proxy()` fallback in `build_reqwest_client` stays only for clients with no proxy set.
6. **A ratchet test** fails on any `reqwest::Client::builder()`, `Client::new()` or `ClientBuilder::new()`
   outside the constructor. Local-only connectors (macos, apple_*, spotlight, whatsapp's local bridge) are
   exempt by name, in one list in that test.

Out of scope: storing or choosing profiles. The host (the RZN backend, or `rzn-tools config set`) puts
`proxy_url` into the auth details; the connector only applies it.

## Done when

- The ratchet test passes and lists no exceptions besides the local-only connectors.
- A test starts a local HTTP proxy, points a connector's client at it, and sees the request arrive. Another
  test checks that an unreachable proxy fails instead of going direct.
- Reddit's and Hacker News's existing `rejects_invalid_proxy_url` tests still pass against the shared code.
- From a Hetzner box through a SOCKS tunnel to a home network, `youtube/get` returns a transcript for a
  video in playlist `PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab`.
