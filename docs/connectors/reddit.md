# Reddit Connector

Public subreddit browsing, post/comment reads, user metadata, and media URL resolution.

## Tools

| Tool | Purpose |
|---|---|
| `list` | Browse subreddit feeds with `sort=hot\|new\|top`. Supports `limit`, `cursor`, `output_format`, and `include_nsfw`. |
| `search` | Search Reddit posts by query, with optional subreddit/author filters. |
| `get` | Fetch a post and comments by URL, item ref, or post id. |
| `media` | Resolve ordered post media URLs tagged as `image`, `animated`, `video`, or `external`. |
| `user` | Fetch public user profile metadata from `about.json`. |

## CLI Examples

```bash
rzn-tools reddit top --subreddit wallpapers --time all --limit 500 --output-format normalized_v1
rzn-tools reddit top --subreddit wallpapers --time all --cursor "$CURSOR" --output-format normalized_v1
rzn-tools reddit hot --subreddit pics --include-nsfw --limit 100
rzn-tools reddit media --id reddit:post:abc123
```

## Listing Output

Raw `list` results include media-archiver fields that Reddit exposes in listing JSON:

- stable ids: `id`, `name`
- media flags: `is_video`, `is_gallery`, `over_18`, `post_hint`
- raw media payloads: `gallery_data`, `media_metadata`, `media`, `secure_media`, `preview`, `crosspost_parent_list`
- link context: `domain`, `url`, `url_overridden_by_dest`, `permalink`
- convenience `resolved_media` entries for gallery/direct/video/crosspost/external media

For pagination, use `output_format=normalized_v1`; the normalized page returns top-level `next_cursor` and `has_more`.

## Access Notes

Anonymous Reddit JSON access can be rate-limited or IP-blocked. The connector uses the configured `proxy_url` for direct JSON calls and can target `old.reddit.com` via:

```bash
rzn-tools config set reddit --key api_base_url --value https://old.reddit.com
```

You can also set `RZN_REDDIT_API_BASE_URL=https://old.reddit.com` for local runs.
