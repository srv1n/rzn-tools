# xAI Search connector (`xai-search`)

Use `xai-search` only for xAI search tools:

- `web_search`
- `x_search`

This connector is intentionally not a generic Grok/chat connector.

## Auth

Required:

- `api_key`

Optional:

- `model`

Environment variable:

- `XAI_API_KEY`

## How To Get An xAI API Key

Official docs:

- [Quickstart](https://docs.x.ai/developers/quickstart)
- [X Search tool](https://docs.x.ai/developers/tools/x-search)
- [Models and pricing](https://docs.x.ai/developers/models)

Practical steps:

1. Create or sign in to your xAI account.
2. Add credits in the xAI console. The quickstart requires funded credits before API use.
3. Open the API Keys page.
4. Create an API key.
5. Store it as `XAI_API_KEY` or import it into rzn-tools as `api_key`.

CLI examples:

```bash
export XAI_API_KEY="<your_xai_api_key>"

# or persist it in the rzn-tools auth store
rzn-tools config set xai-search --key api_key --value "<your_xai_api_key>"
```

Validation:

```bash
rzn-tools xai-search search --query "latest Rust release" --source web
rzn-tools xai-search search --query "OpenAI" --source x
```

## What it does

`xai-search/search` calls the xAI Responses API with search tools attached and returns:

- answer text
- citations
- usage metadata when present

Sources:

- `web`
- `x`

## Search parity supported

Web search:

- `include_domains`
- `exclude_domains`
- normal search/date filters

X search:

- `allowed_x_handles`
- `excluded_x_handles`
- `from_date`
- `to_date`
- `enable_image_understanding`
- `enable_video_understanding`

## Important boundary

`x_search` is not the X platform API.

- use `x` for timelines, posts, likes, follows, bookmarks, and other direct platform operations
- use `xai-search` when you want model-mediated search over the web or X with citations

## Pricing note

xAI bills search tool calls separately from model tokens. Public docs currently list `web_search`
and `x_search` at `$5 / 1k calls`, plus token usage.

So “search-only” still goes through the Responses API and still incurs both tool and token costs.
