# Lightweight YouTube Transcripts

## User story

As an `rzn-tools` user, I can fetch a transcript from a known YouTube URL or video ID in the
default CLI build without compiling a JavaScript engine or multiple HTML parser stacks, while
retaining the current raw and `normalized_v1` output contracts.

## Why this is feasible

The YouTube connector already implements the lightweight happy path locally:

- watch-page retrieval with the shared Reqwest client;
- caption-track extraction;
- preferred-language selection;
- JSON3 timed-text parsing; and
- a bounded `yt-dlp` fallback.

`rusty_ytdl` is currently needed by `get` for rich video metadata and by `search`; the separate
`yt-transcript-rs` path duplicates transcript work before falling back to the local implementation.

## Scope

1. Make known-video `get` use the existing watch-page/timed-text implementation first.
2. Parse only the metadata required by the current response contract from the watch-page player
   JSON: title, description, channel identity, publication date, duration, and chapters.
3. Remove `yt-transcript-rs` from the connector after fixture parity is proven.
4. Remove `rusty_ytdl` and its Boa dependency from the default `youtube` feature.
5. Preserve discovery without burdening transcript users:
   - keep direct channel/playlist listing on the existing Innertube implementation;
   - either implement search with the same Innertube primitives or temporarily gate the current
     `rusty_ytdl` search path behind `youtube-rich`.

## Acceptance

- A1: Default CLI builds still include `youtube`, and `youtube get <URL-or-ID>` returns a transcript
  for a captioned fixture/live smoke video.
- A2: Manual captions, generated captions, unavailable captions, age-restricted/unavailable video,
  and malformed watch-page responses retain typed, stable outcomes.
- A3: Concise, detailed, and `normalized_v1` outputs retain their documented fields and transcript
  block/reference semantics.
- A4: Fixture tests cover player JSON, caption-track selection, JSON3 parsing, chapters, and error
  classification without network access.
- A5: A non-CI live smoke covers one manual-caption video, one generated-caption video, and one
  captions-disabled video; results are recorded separately from deterministic test proof.
- A6: `cargo tree` for the default CLI excludes `boa_engine`, `rusty_ytdl`, and
  `yt-transcript-rs`. No new parser or HTTP dependency is added.
- A7: A clean Make-based default CLI check is timed before and after; the receipt reports dependency
  count and wall time without promising a fixed machine-independent percentage.
- A8: `youtube search` is either preserved with the lightweight implementation or emits an explicit
  compile-time feature hint for `youtube-rich`; it never silently disappears.

## Non-goals

- Video or audio downloading, format deciphering, or media streaming.
- Reimplementing YouTube's player JavaScript.
- Making `yt-dlp` a required runtime dependency.
- Removing YouTube from the default connector set.

## Suggested delivery order

1. Fixture-lock the existing watch-page and JSON3 parsers.
2. Switch transcript retrieval and required metadata to those parsers.
3. Remove `yt-transcript-rs`.
4. Split or replace the remaining `rusty_ytdl` search path.
5. Prove the default dependency graph and run the bounded live smoke.
