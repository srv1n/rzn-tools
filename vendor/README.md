# Vendored dependencies

The workspace `[patch.crates-io]` table selects four local crates:

| Crate | Directory | Use |
| --- | --- | --- |
| `yt-transcript-rs` | `vendor/yt-transcript-rs` | YouTube transcript client with the local portability patch. |
| `rusty_ytdl` | `vendor/rusty_ytdl` | YouTube metadata and stream support with project fixes. |
| `grammers-session` | `vendor/grammers-session` | Telegram session support for the optional desktop profile. |
| `agent-twitter-client` | `vendor/agent-twitter-client` | Browser-session X support for the optional desktop profile. |

Read `vendor/yt-transcript-rs/VENDORING.md` for the transcript patch boundary.
The root `Cargo.toml` is the source of truth for active patches.

Do not edit a vendored crate without recording why the upstream crate is not
enough and adding the smallest check that protects the patch.
