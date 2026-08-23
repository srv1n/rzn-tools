# Smart resolver

`rzn-tools fetch` matches a URL or known ID and selects a connector tool.
The resolver rules are in `rzn_tools_core/src/resolver.rs`.

```bash
rzn-tools fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'
rzn-tools fetch hn:38500000
rzn-tools fetch PMID:12345678
rzn-tools fetch arXiv:2301.07041
rzn-tools fetch https://github.com/rust-lang/rust/issues/12345
rzn-tools formats
```

The resolver has its own priority-ordered regular-expression table. It does not
read `Connector::url_patterns()`. Connector URL patterns are discovery
metadata. Update both places when a route changes.

Current route families include:

- YouTube videos, playlists, channels, and IDs
- Hacker News items
- arXiv, PubMed, DOI, Semantic Scholar, bioRxiv, and medRxiv IDs
- Wikipedia pages
- GitHub repositories, issues, and pull requests
- Reddit posts, users, and communities
- Polymarket and Kalshi pages
- Play Store app pages
- X posts, profiles, and handles
- RSS or Atom feed URLs
- Discord message URLs
- Spotlight and local file inputs
- generic web URLs

Use `rzn-tools formats` or `rzn-tools patterns` for the live list and target
tool names.

## Ambiguous input

An input can match more than one route. Pretty output can ask the user to
choose when results are close in priority. Machine output selects the
highest-priority match.

Use an explicit prefix to avoid common ambiguity:

- `hn:<id>`
- `PMID:<id>`
- `arXiv:<id>`

## Output

`fetch --output-format` forwards `raw`, `normalized_v1`, or `display_v1`
only when the selected tool schema supports that field. Global `--output`
still controls the CLI envelope.
