# Item output (`rzn.item/v2`)

Use this when a connector feeds an index, or when reviewing `output_format=item_v2`. The types are in
`rzn_tools_core/src/item.rs`; the design is `docs/plans/any-source/design.md` in the backend repo.

`item_v2` is opt-in. Tools return raw output by default.

## Shapes

- A get returns one `rzn.item/v2`: `key`, `kind`, `url`, `title`, `author`, `published_at`,
  `published_in`, `marker`, `fields`, `counts`, and a flat ordered list of `blocks` (or a
  `document` pointer for document-like sources, never both).
- A list or search returns `rzn.items/v2`: `items` (listing entries: key, kind and url required, the
  rest when the listing has it), `next_cursor`, and `listing` (`full` when the pages name everything in
  scope, `feed` otherwise; when in doubt, `feed`).
- A `describe` tool returns `rzn.describe/v2`: per item kind, the identity rule, listing type, scopes,
  list and get tools, and every field with `of` (item or passage), `type`, `list`, `changes`.

## Rules

- `key` is the identity, one rule per kind (`youtube:video:<id>`, `reddit:thread:<id>`,
  `hackernews:item:<id>`). The same item through two tools has the same key.
- A full item needs `author`, `published_at` (RFC 3339) and `published_in`; `Item::check` enforces
  it. Use `item::rfc3339` to normalise source dates.
- `counts` holds numbers that keep changing; nothing hourly goes into `fields` or block text.
- A block has `id`, `kind`, `text`, and optionally `path` (headings above it), `parent` (an earlier
  block it answers), `at` (`time` in ms, `chars`, or `order`), `url`, `author`, `published_at`,
  `fields`, `counts`, `attachments`. A `transcript` block must have `at.time`.
- If the connector returned fewer blocks than the item has, set `truncated`; an index refuses it.
- Keep the connector's mapping thin: no per-source logic belongs downstream.
- Put the payload in `structuredContent` with `utils::structured_result`.

## Schema advertising

Declare `output_format` with `ingest::output_format_property()`. Tools that page also declare
`cursor` and `limit`. `item::item_schema()`, `items_page_schema()` and `description_schema()` are
the published JSON Schemas.
