# Federated search

Federated search runs several connector searches and merges the results.

```bash
rzn-tools search "CRISPR gene therapy" --profile research
rzn-tools search "release notes" --sources github,slack
rzn-tools search "query" --profile web --add wikipedia --exclude tavily-search
```

Built-in profiles are defined in
`rzn_tools_core/src/federated/profiles.rs`:

| Profile | Sources |
| --- | --- |
| `research` | `pubmed`, `arxiv`, `semantic-scholar`, `google-scholar` |
| `enterprise` | `slack`, `atlassian`, `github` |
| `social` | `reddit`, `hackernews` |
| `code` | `github` |
| `web` | `perplexity-search`, `exa`, `tavily-search`, `parallel-search` |
| `media` | `youtube`, `wikipedia` |

User profiles are loaded from `profiles.yaml` below the platform config
directory. Profiles can extend another profile and add or exclude sources.
They can also set source weights, limits, response format, and overrides.

The engine runs available sources in parallel. A missing compiled connector is
skipped. Each source has a timeout. Failed sources appear in the error list and
successful sources can still return results.

`grouped` keeps one result group per source. `interleaved` mixes results by
rank and source weight. The config types contain global-timeout and
deduplication settings, but the current engine does not apply those settings.

See [system/cli.md](system/cli.md) for the command contract.
