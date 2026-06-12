use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "rzn-tools")]
#[command(about = "rzn-tools - Unified data access CLI for 30+ sources")]
#[command(version)]
#[command(after_help = "\x1b[1;36mQuick Start:\x1b[0m
  rzn-tools list                              List all available connectors
  rzn-tools tools                             Show all tools with auth requirements
  rzn-tools tools youtube                     Show tools for a specific connector
  rzn-tools search youtube \"rust tutorial\"    Search YouTube videos
  rzn-tools hackernews search --query \"rust\"  Search Hacker News directly
  rzn-tools configure cloudflare guide        Show Cloudflare tunnel setup help
  rzn-tools serve                             Run the local MCP HTTP server
  rzn-tools configure cloudflare tunnel --hostname rzn-tools-origin.example.com --tunnel-name rzn-tools-mcp

\x1b[1;36mAuthentication:\x1b[0m
  rzn-tools setup                             Interactive setup wizard
  rzn-tools setup slack                       Configure a specific connector
  rzn-tools config show                       View current auth configuration
  rzn-tools config test github                Test authentication

\x1b[1;36mMore Info:\x1b[0m
  rzn-tools <command> --help                  Get help for any command
  https://github.com/srv1n/rzn-tools          Full documentation")]
#[command(long_about = "
\x1b[1mrzn-tools\x1b[0m - Unified Data Access CLI

Access 30+ data sources through a single interface:
  • Social: YouTube, Reddit, Hacker News, X/Twitter
  • Academic: arXiv, PubMed, Semantic Scholar
  • Productivity: Slack, GitHub, Atlassian, Microsoft 365, Google Workspace
  • Search: OpenAI, Anthropic, Perplexity, Exa, Tavily, Serper, and more

All connectors expose their capabilities as \x1b[1mtools\x1b[0m. Use `rzn-tools tools` to see
what's available and their authentication requirements.
")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Launch interactive TUI mode
    #[arg(long, global = true)]
    pub tui: bool,

    /// Output format
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Pretty)]
    pub output: OutputFormat,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Verbose output
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Copy output to clipboard
    #[arg(short, long, global = true)]
    pub copy: bool,

    /// Authentication profile to use for connector credentials
    ///
    /// This allows configuring multiple accounts for the same connector, e.g.:
    /// `rzn-tools --auth-profile work setup reddit`
    /// `rzn-tools --auth-profile work config set hackernews --auth-type proxy --value http://127.0.0.1:8080`
    ///
    /// If omitted, rzn-tools uses `default` when present; otherwise it falls back to the first
    /// configured profile for each connector.
    #[arg(long, global = true, value_name = "NAME")]
    pub auth_profile: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all available connectors (data sources)
    ///
    /// Shows a table of all connectors with their descriptions and auth status.
    #[command(alias = "ls")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools list                    Show all connectors
  rzn-tools list --output json      Output as JSON")]
    List,

    /// Interactive setup wizard for configuring authentication
    ///
    /// Run without arguments for guided setup, or specify a connector name
    /// to configure it directly.
    #[command(alias = "init")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools setup                   Start interactive wizard
  rzn-tools setup slack             Configure Slack directly
  rzn-tools setup github            Configure GitHub token")]
    Setup {
        /// Connector name to configure (omit for interactive wizard)
        connector: Option<String>,
    },

    /// Configure hosting and proxy integration helpers
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools configure cloudflare guide
  rzn-tools configure cloudflare doctor
  rzn-tools configure cloudflare tunnel --hostname rzn-tools-origin.example.com --tunnel-name rzn-tools-mcp")]
    Configure {
        #[command(subcommand)]
        target: ConfigureTarget,
    },

    /// Run the native MCP HTTP server
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools configure cloudflare guide
  rzn-tools configure cloudflare tunnel --hostname rzn-tools-origin.example.com --tunnel-name rzn-tools-mcp
  rzn-tools serve
  rzn-tools serve --list-connectors
  rzn-tools serve --add-connectors wikipedia
  rzn-tools serve --remove-connectors reddit
  rzn-tools serve --connectors youtube,hackernews,pubmed,reddit
  rzn-tools serve --all-connectors
  rzn-tools serve --local-only
  rzn-tools serve --bind 127.0.0.1:9000
  rzn-tools serve --allow-hosts localhost,127.0.0.1,rzn-tools-origin.example.com")]
    Serve {
        /// Bind address for the local HTTP server
        #[arg(long)]
        bind: Option<String>,
        /// Comma-separated host allowlist
        #[arg(long, value_delimiter = ',')]
        allow_hosts: Vec<String>,
        /// Replace the exposed connector allowlist
        #[arg(long, value_delimiter = ',')]
        connectors: Vec<String>,
        /// Add connectors to the persisted allowlist
        #[arg(long, value_delimiter = ',')]
        add_connectors: Vec<String>,
        /// Remove connectors from the persisted allowlist
        #[arg(long, value_delimiter = ',')]
        remove_connectors: Vec<String>,
        /// Expose every compiled connector instead of the default allowlist
        #[arg(long)]
        all_connectors: bool,
        /// Show configured and available connectors, then exit
        #[arg(long)]
        list_connectors: bool,
        /// Disable Cloudflare tunnel auto-start and serve only on localhost
        #[arg(long)]
        local_only: bool,
    },

    /// Search for content across connectors
    ///
    /// Search a single connector or multiple connectors simultaneously using profiles.
    #[command(after_help = "\x1b[1;33mSingle Connector:\x1b[0m
  rzn-tools search youtube \"rust programming\"
  rzn-tools search hackernews \"async rust\" --limit 5
  rzn-tools search arxiv \"machine learning\"

\x1b[1;33mFederated Search (Multiple Connectors):\x1b[0m
  rzn-tools search \"CRISPR gene therapy\" --profile research
  rzn-tools search \"release notes\" -s slack,confluence,google-drive
  rzn-tools search \"attention mechanisms\" -p research --merge interleaved

\x1b[1;33mBuilt-in Profiles:\x1b[0m
  research    - pubmed, arxiv, semantic-scholar, google-scholar
  enterprise  - slack, atlassian, github
  social      - reddit, hackernews
  code        - github
  web         - perplexity, exa, tavily
  media       - youtube, wikipedia")]
    Search {
        /// The connector to use (e.g., youtube, reddit) OR the search query when using --profile/-s
        connector_or_query: String,
        /// The search query (optional when using --profile or -s, as first arg becomes the query)
        query: Option<String>,
        /// Maximum number of results per source
        #[arg(short, long, default_value_t = 10)]
        limit: u32,
        /// Search profile for federated search (research, enterprise, social, code, web)
        #[arg(short, long)]
        profile: Option<String>,
        /// Comma-separated list of connectors for ad-hoc federated search
        #[arg(short = 's', long = "sources")]
        connectors: Option<String>,
        /// Merge mode for federated results: grouped (default) or interleaved
        #[arg(short, long, default_value = "grouped")]
        merge: String,
        /// Add connectors to profile (use with --profile)
        #[arg(long)]
        add: Option<String>,
        /// Exclude connectors from profile (use with --profile)
        #[arg(long)]
        exclude: Option<String>,
    },

    /// Get specific content by ID
    ///
    /// Fetches detailed information for a specific resource.
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools get youtube dQw4w9WgXcQ         Get video details + transcript
  rzn-tools get youtube \"$PLAYLIST_URL\"      Enumerate playlist videos
  rzn-tools get youtube dQw4w9WgXcQ --field transcript --output text
  rzn-tools get hackernews 12345            Get HN story with comments
  rzn-tools get arxiv 2301.07041            Get paper details")]
    Get {
        /// The connector to use
        connector: String,
        /// The resource ID or URL
        id: String,
        /// Return a single top-level field from the resource payload
        #[arg(long)]
        field: Option<String>,
    },

    /// Fetch content by automatically detecting the URL or ID type
    ///
    /// Paste any supported URL or ID and rzn-tools will route it to the right connector.
    #[command(alias = "f")]
    #[command(after_help = "\x1b[1;33mSupported Inputs:\x1b[0m
  YouTube:       https://youtube.com/watch?v=xxx, youtu.be/xxx, video ID
  Hacker News:   https://news.ycombinator.com/item?id=xxx, hn:12345678
  ArXiv:         https://arxiv.org/abs/xxx, arXiv:2301.07041
  PubMed:        https://pubmed.ncbi.nlm.nih.gov/xxx, PMID:12345678
  GitHub:        https://github.com/owner/repo, owner/repo
  Reddit:        https://reddit.com/r/xxx, r/rust, https://reddit.com/user/xxx
  X/Twitter:     https://x.com/user/status/xxx, @username
  Wikipedia:     https://en.wikipedia.org/wiki/xxx
  DOI:           https://doi.org/10.xxx, 10.1234/example
  Any URL:       Falls back to web scraper

\x1b[1;33mExamples:\x1b[0m
  rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ
  rzn-tools fetch arXiv:2301.07041
  rzn-tools fetch PMID:12345678
  rzn-tools fetch rust-lang/rust
  rzn-tools fetch r/rust")]
    Fetch {
        /// URL or ID to fetch (auto-detected)
        input: String,
        /// Connector response format: raw, normalized_v1, display_v1 (only applied when the target tool supports it)
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },

    /// Show all supported URL/ID patterns for auto-detection
    #[command(alias = "patterns")]
    Formats,

    /// Manage configuration and authentication
    ///
    /// Set, view, test, or remove authentication credentials for connectors.
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools config show                     Show all saved credentials
  rzn-tools config set slack --value xoxb-xxx
  rzn-tools config set github --value ghp_xxx
  rzn-tools config test slack               Test Slack authentication
  rzn-tools config remove reddit            Remove Reddit credentials")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show detailed information about connectors
    ///
    /// Lists connectors with their tools, auth requirements, and examples.
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools connectors              Show all connector details")]
    Connectors,

    /// List available tools with auth requirements
    ///
    /// Shows all tools across connectors, or tools for a specific connector.
    /// Each tool shows its parameters, whether auth is required, and examples.
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools tools                   List ALL tools from all connectors
  rzn-tools tools youtube           Show YouTube-specific tools
  rzn-tools tools slack             Show Slack tools (requires auth)
  rzn-tools tools --output json     Output as JSON for scripting")]
    Tools {
        /// Connector name to filter tools (omit to show all)
        connector: Option<String>,
    },

    /// Manage ingestion sources and run scheduled ingestion
    ///
    /// Discover ingestion-ready tools, configure sources, and run the ingestion loop.
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools ingest sources
  rzn-tools ingest add reddit:list --args '{\"subreddit\":\"rust\"}'
  rzn-tools ingest list
  rzn-tools ingest run --max-pages 1")]
    Ingest {
        #[command(subcommand)]
        action: IngestAction,
    },

    /// Show pricing info for tools (if available)
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools pricing                 List all pricing entries
  rzn-tools pricing exa             Pricing for all Exa tools
  rzn-tools pricing exa search      Pricing for Exa search tool
  rzn-tools pricing openai-search search --model o4-mini")]
    Pricing {
        /// Connector name to filter (optional)
        connector: Option<String>,
        /// Tool name to filter (optional)
        tool: Option<String>,
        /// Filter by model (optional)
        #[arg(long)]
        model: Option<String>,
    },

    /// Show usage totals (overall or filtered)
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools usage                   Show overall usage totals
  rzn-tools usage --last            Show usage for the most recent run
  rzn-tools usage --run run-123     Show usage for a specific run
  rzn-tools usage exa search        Show usage for Exa search tool")]
    Usage {
        /// Connector name to filter (optional)
        connector: Option<String>,
        /// Tool name to filter (optional)
        tool: Option<String>,
        /// Filter by run id
        #[arg(long)]
        run: Option<String>,
        /// Show only the most recent run
        #[arg(long, conflicts_with = "run")]
        last: bool,
    },

    /// Print a sanitized flow failure draft for the host to submit
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools report tool-broken --connector youtube --tool search --error \"HTTP 429\" --flow-version 0.2.17
  rzn-tools report tool-broken --connector web --tool scrape --error \"invalid JSON\" --flow-version 0.2.17 --note \"selector changed\"")]
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },

    /// Inspect and update bundled starter workflows/examples
    #[command(alias = "systems")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools workflows list
  rzn-tools workflows sync
  rzn-tools workflows sync --remote
  rzn-tools workflows sync --remote --version v0.2.17")]
    Workflows {
        #[command(subcommand)]
        action: WorkflowAction,
    },

    /// Install the bundled rzn-tools Agent Skill into agent clients
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools skills install --scope project
  rzn-tools skills install --scope global --clients claude,codex
  rzn-tools skills update --scope global --clients all
  rzn-tools skills status --scope project
  rzn-tools skills remove --scope global --clients gemini")]
    Skills {
        #[command(subcommand)]
        action: SkillAction,
    },

    // ========================================================================
    // Connector-specific subcommands with proper CLI flags
    // ========================================================================
    /// OpenAI web search
    #[command(name = "openai-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools openai-search search --query \"rust async programming\"
  rzn-tools openai-search search --query \"AI news\" --max-results 10")]
    OpenaiSearch {
        #[command(subcommand)]
        tool: OpenaiSearchTools,
    },

    /// Anthropic web search
    #[command(name = "anthropic-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools anthropic-search search --query \"rust async programming\"
  rzn-tools anthropic-search search --query \"AI news\" --max-results 10")]
    AnthropicSearch {
        #[command(subcommand)]
        tool: AnthropicSearchTools,
    },

    /// Gemini web search
    #[command(name = "gemini-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools gemini-search search --query \"rust async programming\"
  rzn-tools gemini-search search --query \"AI news\" --max-results 10")]
    GeminiSearch {
        #[command(subcommand)]
        tool: GeminiSearchTools,
    },

    /// Perplexity web search
    #[command(name = "perplexity-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools perplexity-search search --query \"rust async programming\"
  rzn-tools perplexity-search search --query \"AI news\" --max-results 10")]
    PerplexitySearch {
        #[command(subcommand)]
        tool: PerplexitySearchTools,
    },

    /// xAI web search
    #[command(name = "xai-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools xai-search search --query \"rust async programming\"
  rzn-tools xai-search search --query \"AI news\" --max-results 10")]
    XaiSearch {
        #[command(subcommand)]
        tool: XaiSearchTools,
    },

    /// Exa neural search
    #[command(name = "exa")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools exa search --query \"rust async programming\" --num-results 10
  rzn-tools exa find-similar --url https://example.com
  rzn-tools exa answer --query \"What is Rust?\"
  rzn-tools exa get-contents --ids url1,url2")]
    Exa {
        #[command(subcommand)]
        tool: ExaTools,
    },

    /// Tavily web search
    #[command(name = "tavily-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools tavily-search search --query \"rust async programming\"
  rzn-tools tavily-search search --query \"AI news\" --max-results 10 --depth advanced")]
    TavilySearch {
        #[command(subcommand)]
        tool: TavilySearchTools,
    },

    /// Serper web search
    #[command(name = "serper-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools serper-search search --query \"rust async programming\"
  rzn-tools serper-search search --query \"AI news\" --max-results 10")]
    SerperSearch {
        #[command(subcommand)]
        tool: SerperSearchTools,
    },

    /// SerpAPI web search
    #[command(name = "serpapi-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools serpapi-search search --query \"rust async programming\"
  rzn-tools serpapi-search search --query \"AI news\" --max-results 10 --engine google")]
    SerpapiSearch {
        #[command(subcommand)]
        tool: SerpapiSearchTools,
    },

    /// Firecrawl search and scraping
    #[command(name = "firecrawl-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools firecrawl-search search --query \"rust async programming\"
  rzn-tools firecrawl-search search --query \"AI news\" --scrape false")]
    FirecrawlSearch {
        #[command(subcommand)]
        tool: FirecrawlSearchTools,
    },

    /// Parallel AI web search
    #[command(name = "parallel-search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools parallel-search search --query \"rust async programming\"
  rzn-tools parallel-search search --query \"AI news\" --max-results 10")]
    ParallelSearch {
        #[command(subcommand)]
        tool: ParallelSearchTools,
    },

    /// Google Calendar events and management
    #[command(name = "google-calendar", alias = "gcal")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-calendar list-events
  rzn-tools google-calendar create-event --summary \"Meeting\" --start \"2025-01-01T10:00:00Z\" --end \"2025-01-01T11:00:00Z\"
  rzn-tools google-calendar update-event --event-id abc123 --summary \"Updated Meeting\"
  rzn-tools google-calendar delete-event --event-id abc123")]
    GoogleCalendar {
        #[command(subcommand)]
        tool: GoogleCalendarTools,
    },

    /// CalDAV calendar integration (iCloud, Fastmail, Nextcloud, Radicale, etc.)
    #[command(name = "caldav")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools caldav list-calendars
  rzn-tools caldav list-events --limit 20 --output-format normalized_v1
  rzn-tools caldav get-event --item-ref \"caldav:event:<base64url>\"
  rzn-tools caldav create-event --summary \"Team Sync\" --start \"2026-02-21T15:00:00Z\" --end \"2026-02-21T15:30:00Z\"
  rzn-tools caldav update-event --url \"https://.../event.ics\" --summary \"Updated title\"
  rzn-tools caldav delete-event --item-ref \"caldav:event:<base64url>\"")]
    Caldav {
        #[command(subcommand)]
        tool: CaldavTools,
    },

    /// Google Drive file management
    #[command(name = "google-drive", alias = "gdrive")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-drive list-files
  rzn-tools google-drive get-file --file-id abc123
  rzn-tools google-drive download-file --file-id abc123
  rzn-tools google-drive export-file --file-id abc123 --mime-type application/pdf")]
    GoogleDrive {
        #[command(subcommand)]
        tool: GoogleDriveTools,
    },

    /// Google Gmail messages and threads
    #[command(name = "google-gmail", alias = "gmail")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-gmail list-messages
  rzn-tools google-gmail list-messages --q \"from:example@gmail.com\"
  rzn-tools google-gmail get-message --id abc123
  rzn-tools google-gmail get-thread --id abc123")]
    GoogleGmail {
        #[command(subcommand)]
        tool: GoogleGmailTools,
    },

    /// Google People contacts
    #[command(name = "google-people", alias = "gpeople")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-people list-connections
  rzn-tools google-people get-person --resource-name people/c123")]
    GooglePeople {
        #[command(subcommand)]
        tool: GooglePeopleTools,
    },

    /// Google Search Console (SEO performance + indexing status)
    #[command(name = "google-search-console", alias = "gsc")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-search-console list-sites
  rzn-tools google-search-console search-analytics --site-url https://example.com/ --start-date 2026-01-01 --end-date 2026-01-31 --dimensions query
  rzn-tools google-search-console list-sitemaps --site-url https://example.com/
  rzn-tools google-search-console inspect-url --site-url https://example.com/ --inspection-url https://example.com/page")]
    GoogleSearchConsole {
        #[command(subcommand)]
        tool: GoogleSearchConsoleTools,
    },

    /// Bing Webmaster Tools (SEO performance + URL submission)
    #[command(name = "bing-webmaster-tools", alias = "bwt", alias = "bing-webmaster")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools bing-webmaster-tools list-sites
  rzn-tools bing-webmaster-tools get-query-stats --site-url https://example.com/
  rzn-tools bing-webmaster-tools submit-url --site-url https://example.com/ --url https://example.com/new-page")]
    BingWebmasterTools {
        #[command(subcommand)]
        tool: BingWebmasterToolsTools,
    },

    /// LinkedIn official OAuth/OIDC APIs (token import only, no browser automation)
    #[command(name = "linkedin")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools setup linkedin
  rzn-tools linkedin auth-status
  rzn-tools linkedin me
  rzn-tools linkedin share --text \"Hello LinkedIn\"
  rzn-tools linkedin company-share --organization urn:li:organization:123 --text \"Company update\"
  rzn-tools linkedin api-request --method GET --path /v2/userinfo")]
    Linkedin {
        #[command(subcommand)]
        tool: LinkedinTools,
    },

    /// Google Scholar paper search
    #[command(name = "google-scholar", alias = "gscholar")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools google-scholar search-papers --query \"CRISPR gene therapy\"
  rzn-tools google-scholar search-papers --query \"machine learning\" --limit 20")]
    GoogleScholar {
        #[command(subcommand)]
        tool: GoogleScholarTools,
    },

    /// Atlassian (Jira + Confluence)
    #[command(name = "atlassian", alias = "jira")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools atlassian test-auth
  rzn-tools atlassian jira-search --jql \"project = DEMO AND status = Open\"
  rzn-tools atlassian jira-get --key DEMO-123
  rzn-tools atlassian conf-search --cql \"type = page AND space = DEMO\"
  rzn-tools atlassian conf-get --id 123456")]
    Atlassian {
        #[command(subcommand)]
        tool: AtlassianTools,
    },

    /// Microsoft Graph (Microsoft 365)
    #[command(name = "microsoft-graph", alias = "msgraph")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools microsoft-graph list-messages --top 20
  rzn-tools microsoft-graph list-events --days-ahead 7
  rzn-tools microsoft-graph get-message --message-id ABC123
  rzn-tools microsoft-graph send-mail --to user@example.com --subject \"Hello\" --body \"Test\"
  rzn-tools microsoft-graph create-draft --to user@example.com --subject \"Draft\" --body \"Draft message\"")]
    MicrosoftGraph {
        #[command(subcommand)]
        tool: MicrosoftGraphTools,
    },

    /// IMAP email
    #[command(name = "imap", alias = "email")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools imap list-mailboxes
  rzn-tools imap fetch-messages --mailbox Junk --limit 20
  rzn-tools imap get-message --mailbox Junk --uid 12345
  rzn-tools imap search --mailbox Junk --query \"UNSEEN\"
  rzn-tools imap move-messages --mailbox Junk --destination-mailbox INBOX --uids 12345 --apply
  rzn-tools imap delete-messages --mailbox Junk --uids 12345 --apply")]
    Imap {
        #[command(subcommand)]
        tool: ImapTools,
    },

    /// SMTP outbound email sending
    #[command(name = "smtp", alias = "mailer")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools smtp test-connection
  rzn-tools smtp send-mail --to user@example.com --subject \"Hello\" --body \"Test\"
  rzn-tools smtp send-mail --to alice@example.com,bob@example.com --cc team@example.com --subject \"Status\" --body \"Build passed\"
  rzn-tools smtp send-mail --to user@example.com --subject \"Preview\" --body \"No send\" --dry-run

\x1b[1;33mNotes:\x1b[0m
  - Configure credentials with: rzn-tools setup smtp
  - Use app passwords for Gmail/iCloud/Outlook when MFA is enabled
  - SMTP connector is outbound-only (send + connection test)")]
    Smtp {
        #[command(subcommand)]
        tool: SmtpTools,
    },

    /// Local filesystem text extraction (PDF, EPUB, DOCX, HTML, Markdown, code)
    #[command(name = "localfs", alias = "fs", alias = "file")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools localfs list-files --path ~/Documents --recursive --extensions pdf,md
  rzn-tools localfs extract-text --path ~/paper.pdf
  rzn-tools localfs structure --path ~/book.epub")]
    Localfs {
        #[command(subcommand)]
        tool: LocalfsTools,
    },

    /// YouTube video details, transcripts, and search
    #[command(name = "youtube", alias = "yt")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools youtube search --query \"rust programming\" --limit 10
  rzn-tools youtube list --playlist \"https://youtube.com/playlist?list=PL...\"
  rzn-tools youtube list --channel @hubermanlab --limit 5
  rzn-tools youtube resolve-channel --channel @hubermanlab
  rzn-tools youtube dQw4w9WgXcQ
  rzn-tools youtube get https://youtube.com/watch?v=dQw4w9WgXcQ")]
    Youtube {
        #[command(flatten)]
        args: YoutubeArgs,
    },

    /// Hacker News stories, comments, and search
    #[command(name = "hackernews", alias = "hn")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools hackernews search --query \"rust\" --limit 10
  rzn-tools hackernews story --id 12345678")]
    Hackernews {
        #[command(subcommand)]
        tool: HackernewsTools,
    },

    /// arXiv paper search and retrieval
    #[command(name = "arxiv")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools arxiv search --query \"transformer architecture\" --limit 10
  rzn-tools arxiv paper --id 2301.07041")]
    Arxiv {
        #[command(subcommand)]
        tool: ArxivTools,
    },

    /// GitHub repositories, issues, PRs, and code search
    #[command(name = "github", alias = "gh")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools github search-repos --query \"rust cli\"
  rzn-tools github search-code --query \"async fn\" --repo tokio-rs/tokio")]
    Github {
        #[command(subcommand)]
        tool: GithubTools,
    },

    /// Reddit posts, comments, and subreddit search
    #[command(name = "reddit")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools reddit search --query \"rust\" --subreddit programming
  rzn-tools reddit hot --subreddit rust --limit 20
  rzn-tools reddit post --id https://www.reddit.com/comments/abc123 --comment-limit 200 --comment-sort top
  rzn-tools reddit user --username spez")]
    Reddit {
        #[command(subcommand)]
        tool: RedditTools,
    },

    /// Polymarket discovery and market-analysis workflows
    #[command(name = "polymarket")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools polymarket list-tags --limit 20
  rzn-tools polymarket list-events --tag-slug crypto --active --limit 10
  rzn-tools polymarket market-context --slug cbb-pur-arz-2026-03-28 --include-positions
  rzn-tools fetch https://polymarket.com/event/cbb-pur-arz-2026-03-28")]
    Polymarket {
        #[command(subcommand)]
        tool: PolymarketTools,
    },

    /// Kalshi discovery and market-analysis workflows
    #[command(name = "kalshi")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools kalshi search --query \"fed rates\" --limit 10
  rzn-tools kalshi get-event --ticker KXELONMARS-99
  rzn-tools kalshi market-context --ticker KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3")]
    Kalshi {
        #[command(subcommand)]
        tool: KalshiTools,
    },

    /// Google Play Store app metadata (best-effort scraping)
    #[command(name = "play-store", alias = "playstore")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools play-store app --id com.whatsapp
  rzn-tools fetch \"https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US\"")]
    PlayStore {
        #[command(subcommand)]
        tool: PlayStoreTools,
    },

    /// Public App Store app metadata (iTunes Search API)
    #[command(name = "app-store", aliases = ["appstore", "itunes"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools app-store search --query \"habit tracker\" --limit 10
  rzn-tools app-store lookup --track-id 310633997
  rzn-tools app-store reviews --track-id 310633997
  rzn-tools fetch https://apps.apple.com/us/app/id310633997")]
    AppStore {
        #[command(subcommand)]
        tool: AppStoreTools,
    },

    /// App Store Connect API (apps, App Analytics, Sales/Finance reports)
    #[command(name = "app-store-connect", aliases = ["asc", "appstoreconnect"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools setup app-store-connect
  rzn-tools app-store-connect list-apps --limit 25
  rzn-tools app-store-connect get-app --app-id 123456789
  rzn-tools app-store-connect download-sales-report --report-date 2026-02-01 --frequency MONTHLY

\x1b[1;33mNotes:\x1b[0m
  - Requires App Store Connect API JWT credentials (key_id, issuer_id, private_key_path)")]
    AppStoreConnect {
        #[command(subcommand)]
        tool: AppStoreConnectTools,
    },

    /// Apple Search Ads API v5 (keyword recommendations and reporting)
    #[command(name = "apple-search-ads", aliases = ["asa", "apple-searchads"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools setup apple-search-ads
  rzn-tools apple-search-ads list-campaigns --limit 10
  rzn-tools apple-search-ads keyword-recommendations --app-id 310633997 --storefront-countries US

\x1b[1;33mNotes:\x1b[0m
  - Requires OAuth client credentials + ES256 private key (.p8)")]
    AppleSearchAds {
        #[command(subcommand)]
        tool: AppleSearchAdsTools,
    },

    /// Web page scraping and content extraction
    #[command(name = "web")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools web scrape --url https://example.com")]
    Web {
        #[command(subcommand)]
        tool: WebTools,
    },

    /// Wikipedia article search and retrieval
    #[command(name = "wikipedia", alias = "wiki")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools wikipedia search --query \"Rust programming\"
  rzn-tools wikipedia article --title \"Rust (programming language)\"")]
    Wikipedia {
        #[command(subcommand)]
        tool: WikipediaTools,
    },

    /// PubMed medical literature search
    #[command(name = "pubmed")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools pubmed search --query \"CRISPR gene therapy\" --limit 10
  rzn-tools pubmed article --pmid 12345678")]
    Pubmed {
        #[command(subcommand)]
        tool: PubmedTools,
    },

    /// Semantic Scholar academic paper search
    #[command(name = "semantic-scholar", alias = "scholar")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools semantic-scholar search --query \"attention mechanism\" --limit 10
  rzn-tools semantic-scholar paper --id abc123")]
    SemanticScholar {
        #[command(subcommand)]
        tool: SemanticScholarTools,
    },

    /// Slack channels, messages, and search
    #[command(name = "slack")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools slack channels
  rzn-tools slack messages --channel general --limit 50")]
    Slack {
        #[command(subcommand)]
        tool: SlackTools,
    },

    /// X (Twitter) API v2 (bearer token)
    #[command(name = "x", aliases = ["twitter", "x-api", "twitter-api"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools x search --query \"rust lang:en\" --since 12h --quick true --sort-by likes
  rzn-tools x tweet --tweet-id 1234567890123456789
  rzn-tools x profile --username TwitterDev
  rzn-tools x thread --tweet-id 1234567890123456789 --pages 3 --order asc")]
    X {
        #[command(subcommand)]
        tool: XApiTools,
    },

    /// X (Twitter) via browser cookies (scraper-based)
    #[command(name = "x-browser", aliases = ["x-cookies", "twitter-cookies", "twitter-browser"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools x-browser profile --username elonmusk
  rzn-tools x-browser search --query \"rust lang\" --limit 20 --since 2026-02-01 --until 2026-02-24
  rzn-tools x-browser thread --tweet-id 1234567890123456789 --sort-by time --order asc")]
    XBrowser {
        #[command(subcommand)]
        tool: XTools,
    },

    /// Discord servers, channels, and messages
    #[command(name = "discord")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools discord servers
  rzn-tools discord channels --guild-id 123456789")]
    Discord {
        #[command(subcommand)]
        tool: DiscordTools,
    },

    /// RSS feed reader
    #[command(name = "rss")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools rss feed --url https://example.com/feed.xml
  rzn-tools rss entries --url https://example.com/feed.xml --limit 20")]
    Rss {
        #[command(subcommand)]
        tool: RssTools,
    },

    /// bioRxiv and medRxiv preprint search
    #[command(name = "biorxiv")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools biorxiv recent --server biorxiv --count 20
  rzn-tools biorxiv paper --server biorxiv --doi 10.1101/2024.01.01.123456")]
    Biorxiv {
        #[command(subcommand)]
        tool: BiorxivTools,
    },

    /// Open-access paper lookup by DOI (via OpenAlex/Unpaywall)
    ///
    /// Find freely available versions of academic papers by DOI.
    /// Returns PDF URLs, titles, authors, and publication metadata.
    ///
    /// NOTE: Does NOT bypass paywalls - only returns legally available
    /// open-access copies when they exist.
    #[command(name = "scihub")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  \x1b[2m# Look up a paper by DOI\x1b[0m
  rzn-tools scihub paper --doi 10.1371/journal.pone.0000308

  \x1b[2m# Look up a Nature paper\x1b[0m
  rzn-tools scihub paper --doi 10.1038/nature12373

  \x1b[2m# arXiv paper by DOI\x1b[0m
  rzn-tools scihub paper --doi 10.48550/arXiv.1706.03762

  \x1b[2m# Search for papers by topic\x1b[0m
  rzn-tools scihub search --query \"attention mechanism\" --limit 5

  \x1b[2m# Search open-access only\x1b[0m
  rzn-tools scihub search --query \"CRISPR\" --oa-only

  \x1b[2m# Batch lookup multiple DOIs\x1b[0m
  rzn-tools scihub batch --dois \"10.1038/nature12373,10.1371/journal.pone.0000308\"

  \x1b[2m# Output as JSON\x1b[0m
  rzn-tools scihub paper --doi 10.1038/nature12373 --output json

\x1b[1;33mConfiguration (optional, for better results):\x1b[0m
  export UNPAYWALL_EMAIL=\"you@example.com\"
  \x1b[2m# Or: rzn-tools setup scihub\x1b[0m

\x1b[1;33mResponse Fields:\x1b[0m
  pdf_url   Direct PDF link (if found)
  title     Paper title
  authors   Author names
  year      Publication year
  success   true if open-access PDF found

\x1b[1;33mDocs:\x1b[0m docs/connectors/scihub.md")]
    Scihub {
        #[command(subcommand)]
        tool: ScihubTools,
    },

    /// macOS automation and scripting
    #[command(name = "macos")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools macos script --script \"display dialog \\\"Hello\\\"\"
  rzn-tools macos notify --message \"Task complete\"")]
    Macos {
        #[command(subcommand)]
        tool: MacosTools,
    },

    /// Spotlight file search
    #[command(name = "spotlight")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools spotlight search --query \"rust async\"
  rzn-tools spotlight name --name \"cargo.toml\"")]
    Spotlight {
        #[command(subcommand)]
        tool: SpotlightTools,
    },

    /// Apple Messages with privacy-safe aliases
    #[command(name = "apple-messages", aliases = ["imessage", "messages"])]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools apple-messages chats --limit 10
  rzn-tools apple-messages messages --alias msg-001 --since 2026-03-20
  rzn-tools apple-messages send --alias mom --message \"Running late\"
  rzn-tools apple-messages set-alias --alias mom --identifier +15551234567")]
    AppleMessages {
        #[command(subcommand)]
        tool: AppleMessagesTools,
    },
}

#[derive(Subcommand, Clone)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set authentication for a connector
    Set {
        /// Connector name
        connector: String,
        /// Explicit config field key to set (advanced)
        ///
        /// Example: `rzn-tools config set app-store-connect --key issuer_id --value ...`
        #[arg(long)]
        key: Option<String>,
        /// Authentication method (api-key, browser, oauth)
        #[arg(long)]
        auth_type: Option<String>,
        /// API key or credential value
        #[arg(long)]
        value: Option<String>,
        /// Browser to extract cookies from (chrome, firefox, edge, safari, brave)
        #[arg(long)]
        browser: Option<String>,
    },
    /// Remove authentication for a connector
    Remove {
        /// Connector name
        connector: String,
    },
    /// Test authentication for a connector
    Test {
        /// Connector name
        connector: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum IngestAction {
    /// List ingestion-ready sources (connectors/ingest_sources)
    Sources {
        /// Filter by connector names (comma-separated)
        #[arg(long)]
        connectors: Option<String>,
        /// Filter by tool categories (comma-separated)
        #[arg(long)]
        categories: Option<String>,
        /// Include windowed read tools (category=read, supports_cursor=true)
        #[arg(long, default_value_t = true)]
        include_read: bool,
        /// Include one-shot fetch tools (category=read, supports_cursor=false)
        #[arg(long, default_value_t = false)]
        include_fetch: bool,
    },
    /// Add an ingestion source to local config
    Add {
        /// Ingest source id (e.g., reddit:list) or tool name (reddit/list)
        id: String,
        /// JSON arguments for the tool (object)
        #[arg(long)]
        args: Option<String>,
        /// Tenant name (default: "default")
        #[arg(long)]
        tenant: Option<String>,
        /// Disable this source after adding
        #[arg(long, default_value_t = false)]
        disabled: bool,
        /// Optional cadence in seconds
        #[arg(long)]
        cadence_seconds: Option<u64>,
        /// Allow adding one-shot fetch tools (include_fetch=true)
        #[arg(long, default_value_t = false)]
        include_fetch: bool,
    },
    /// List configured ingestion sources
    List {
        /// Tenant name (default: "default")
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Remove a configured ingestion source
    Remove {
        /// Ingest source id (e.g., reddit:list)
        id: String,
        /// Tenant name (default: "default")
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Run the ingestion loop
    Run {
        /// Tenant name (default: "default")
        #[arg(long)]
        tenant: Option<String>,
        /// Run only a specific source id
        #[arg(long)]
        id: Option<String>,
        /// Maximum pages per source per run
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        max_pages: u32,
        /// Stop after indexing this many items (per source)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        max_items: Option<u32>,
        /// Repeat ingestion every N seconds
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        interval_seconds: Option<u64>,
        /// Include disabled sources
        #[arg(long, default_value_t = false)]
        include_disabled: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum WorkflowAction {
    /// Show workflow/example asset paths and discovered systems
    List,
    /// Sync workflow/example assets into the managed user directory
    Sync {
        /// Pull the workflow bundle from the latest GitHub release instead of the bundled local share dir
        #[arg(long, default_value_t = false)]
        remote: bool,
        /// Specific release version or tag to pull (implies --remote)
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand, Clone)]
pub enum SkillAction {
    /// Show where the bundled skill would be installed and current link status
    Status(SkillArgs),
    /// Install symlinks for the bundled rzn-tools skill
    #[command(alias = "setup")]
    Install(SkillInstallArgs),
    /// Refresh managed skill source and relink selected clients
    Update(SkillInstallArgs),
    /// Remove installed symlinks for selected clients
    Remove(SkillRemoveArgs),
}

#[derive(Args, Clone)]
pub struct SkillArgs {
    /// Install scope
    #[arg(long, value_enum, default_value_t = SkillScope::Project)]
    pub scope: SkillScope,
    /// Client targets, comma-separated: all, claude, gemini, agent, codex
    #[arg(long, value_enum, value_delimiter = ',', default_value = "all")]
    pub clients: Vec<SkillClient>,
}

#[derive(Args, Clone)]
pub struct SkillInstallArgs {
    /// Install scope
    #[arg(long, value_enum, default_value_t = SkillScope::Project)]
    pub scope: SkillScope,
    /// Client targets, comma-separated: all, claude, gemini, agent, codex
    #[arg(long, value_enum, value_delimiter = ',', default_value = "all")]
    pub clients: Vec<SkillClient>,
    /// Skill source preference
    #[arg(long, value_enum, default_value_t = SkillSource::Auto)]
    pub source: SkillSource,
    /// Replace an existing non-matching symlink or directory at the target path
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, Clone)]
pub struct SkillRemoveArgs {
    /// Install scope
    #[arg(long, value_enum, default_value_t = SkillScope::Project)]
    pub scope: SkillScope,
    /// Client targets, comma-separated: all, claude, gemini, agent, codex
    #[arg(long, value_enum, value_delimiter = ',', default_value = "all")]
    pub clients: Vec<SkillClient>,
    /// Also delete the managed embedded skill source used by release installs
    #[arg(long, default_value_t = false)]
    pub delete_source: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum SkillScope {
    /// User-level install
    Global,
    /// Current project install
    Project,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum SkillClient {
    /// Every supported client target
    All,
    /// Claude Code
    Claude,
    /// Gemini CLI
    Gemini,
    /// Generic Agent Skills directory
    Agent,
    /// OpenAI Codex
    Codex,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum SkillSource {
    /// Link to the repo checkout when available, otherwise materialize the embedded release copy
    Auto,
    /// Link to the checked-out repo skill at .agents/skills/rzn-tools
    Repo,
    /// Materialize the skill embedded in this CLI binary and link to that managed copy
    Embedded,
}

#[derive(Subcommand, Clone)]
pub enum ReportAction {
    /// Print a broken connector tool draft without args, logs, or response data
    #[command(name = "tool-broken")]
    ToolBroken(ToolBrokenReportArgs),
}

#[derive(Args, Clone)]
pub struct ToolBrokenReportArgs {
    /// Connector name, for example youtube or web
    #[arg(long)]
    pub connector: String,
    /// Tool name, for example search or scrape
    #[arg(long)]
    pub tool: String,
    /// Raw or stable error text. It is normalized before the draft is printed.
    #[arg(long)]
    pub error: String,
    /// Tool package/catalog version
    #[arg(long = "flow-version")]
    pub flow_version: String,
    /// Optional context written by the user
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable formatted output
    Pretty,
    /// JSON output
    Json,
    /// YAML output
    Yaml,
    /// Plain text output
    Text,
    /// Markdown output
    Markdown,
}

#[derive(Subcommand, Clone)]
pub enum ConfigureTarget {
    /// Save Cloudflare tunnel defaults for `rzn-tools serve`
    Cloudflare {
        #[command(subcommand)]
        action: CloudflareConfigureAction,
    },
}

#[derive(Subcommand, Clone)]
pub enum CloudflareConfigureAction {
    /// Show first-run setup help for rzn-tools behind Cloudflare Tunnel
    Guide,
    /// Inspect your local Cloudflare + rzn-tools setup and call out missing pieces
    Doctor {
        /// Tunnel name to verify with cloudflared
        #[arg(long)]
        tunnel_name: Option<String>,
    },
    /// Configure the local MCP server for a Cloudflare Tunnel hostname
    Tunnel {
        /// Tunnel hostname that should point at your local rzn-tools server
        #[arg(long)]
        hostname: String,
        /// Named tunnel to run with `cloudflared tunnel run <name>`
        #[arg(long)]
        tunnel_name: Option<String>,
        /// Local bind address rzn-tools should listen on
        #[arg(long)]
        bind: Option<String>,
    },
}

// ============================================================================
// Connector-specific tool enums with proper CLI flags
// ============================================================================

/// OpenAI Search tools
#[derive(Subcommand, Clone)]
pub enum OpenaiSearchTools {
    /// Search the web using OpenAI with grounding
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of sources to cite
        #[arg(long, default_value_t = 5, alias = "max-results")]
        limit: u32,
        /// Model name (e.g., o4-mini, gpt-4.1)
        #[arg(long)]
        model: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Anthropic Search tools
#[derive(Subcommand, Clone)]
pub enum AnthropicSearchTools {
    /// Search the web using Claude with grounding
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of sources to cite
        #[arg(long, default_value_t = 5, alias = "max-results")]
        limit: u32,
        /// Model name (e.g., claude-3-7-sonnet-latest)
        #[arg(long)]
        model: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Gemini Search tools
#[derive(Subcommand, Clone)]
pub enum GeminiSearchTools {
    /// Search the web using Gemini with grounding
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of sources to cite
        #[arg(long, default_value_t = 5, alias = "max-results")]
        limit: u32,
        /// Model name (e.g., gemini-1.5-pro-latest)
        #[arg(long)]
        model: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Perplexity Search tools
#[derive(Subcommand, Clone)]
pub enum PerplexitySearchTools {
    /// Search the web using Perplexity with grounding
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of sources to cite
        #[arg(long, default_value_t = 5, alias = "max-results")]
        limit: u32,
        /// Model name (e.g., sonar-pro)
        #[arg(long)]
        model: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// xAI Search tools
#[derive(Subcommand, Clone)]
pub enum XaiSearchTools {
    /// Search the web and X using xAI with grounding
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of sources to cite
        #[arg(long, default_value_t = 5, alias = "max-results")]
        limit: u32,
        /// Model name (e.g., grok-4-fast)
        #[arg(long)]
        model: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Exa tools
#[derive(Subcommand, Clone)]
pub enum ExaTools {
    /// Neural search using embeddings
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Number of results
        #[arg(long, default_value_t = 10, alias = "num-results")]
        limit: u32,
        /// Search type: auto, fast, or deep
        #[arg(long, default_value = "auto")]
        type_: String,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get clean parsed content from URLs
    #[command(name = "get-contents")]
    GetContents {
        /// Comma-separated list of URLs or Exa result IDs
        #[arg(long, short)]
        ids: String,
    },

    /// Find similar pages to a URL
    #[command(name = "find-similar")]
    FindSimilar {
        /// URL to find similar pages for
        #[arg(long, short)]
        url: String,
        /// Number of results
        #[arg(long, default_value_t = 10, alias = "num-results")]
        limit: u32,
    },

    /// Get LLM-generated answer with citations
    #[command(name = "answer")]
    Answer {
        /// Question to answer
        #[arg(long, short)]
        query: String,
        /// Answer mode: precise or detailed
        #[arg(long)]
        mode: Option<String>,
    },
}

/// Tavily Search tools
#[derive(Subcommand, Clone)]
pub enum TavilySearchTools {
    /// Search the web using Tavily
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10, alias = "max-results")]
        limit: u32,
        /// Search depth: basic or advanced
        #[arg(long, default_value = "basic")]
        depth: String,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Serper Search tools
#[derive(Subcommand, Clone)]
pub enum SerperSearchTools {
    /// Search Google via Serper.dev
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10, alias = "max-results")]
        limit: u32,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// SerpAPI Search tools
#[derive(Subcommand, Clone)]
pub enum SerpapiSearchTools {
    /// Search Google via SerpAPI
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10, alias = "max-results")]
        limit: u32,
        /// Search engine: google, bing, etc.
        #[arg(long, default_value = "google")]
        engine: String,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Firecrawl Search tools
#[derive(Subcommand, Clone)]
pub enum FirecrawlSearchTools {
    /// Search and scrape the web using Firecrawl
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10, alias = "max-results")]
        limit: u32,
        /// Whether to scrape and parse content
        #[arg(long, default_value_t = true)]
        scrape: bool,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Parallel Search tools
#[derive(Subcommand, Clone)]
pub enum ParallelSearchTools {
    /// Search the web using Parallel AI
    #[command(name = "search")]
    Search {
        /// Search query or objective
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10, alias = "max-results")]
        limit: u32,
    },
}

// ============================================================================
// Google Connector tools with proper CLI flags
// ============================================================================

/// Google Calendar tools
#[derive(Subcommand, Clone)]
pub enum GoogleCalendarTools {
    /// List upcoming events from primary calendar
    #[command(name = "list-events", alias = "events")]
    ListEvents {
        /// Total number of events to return (1-5000). Connector paginates internally.
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        max_results: u32,
        /// Optional cursor from a previous response (nextPageToken)
        #[arg(long)]
        page_token: Option<String>,
        /// Minimum time (RFC3339 format)
        #[arg(long)]
        time_min: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Create an event in primary calendar
    #[command(name = "create-event", alias = "create")]
    CreateEvent {
        /// Event title/summary
        #[arg(long, short)]
        summary: String,
        /// Start time (RFC3339 format)
        #[arg(long)]
        start: String,
        /// End time (RFC3339 format)
        #[arg(long)]
        end: String,
    },

    /// Incremental sync using syncToken
    #[command(name = "sync-events", alias = "sync")]
    SyncEvents {
        /// Sync token from previous sync
        #[arg(long, short)]
        sync_token: String,
        /// Maximum number of results (1-250)
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=250)
        )]
        max_results: u32,
    },

    /// Update an event in primary calendar
    #[command(name = "update-event", alias = "update")]
    UpdateEvent {
        /// Event ID
        #[arg(long, short)]
        event_id: String,
        /// New event title/summary
        #[arg(long)]
        summary: Option<String>,
        /// New start time (RFC3339 format)
        #[arg(long)]
        start: Option<String>,
        /// New end time (RFC3339 format)
        #[arg(long)]
        end: Option<String>,
    },

    /// Delete an event in primary calendar
    #[command(name = "delete-event", alias = "delete")]
    DeleteEvent {
        /// Event ID
        #[arg(long, short)]
        event_id: Option<String>,
    },
}

/// CalDAV tools
#[derive(Subcommand, Clone)]
pub enum CaldavTools {
    /// Discover available calendars
    #[command(name = "list-calendars", alias = "calendars")]
    ListCalendars {
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// List events from a calendar collection
    #[command(name = "list-events", alias = "list")]
    ListEvents {
        /// Optional calendar URL override
        #[arg(long)]
        calendar_url: Option<String>,
        /// Maximum events to return per call (1-500)
        #[arg(
            long,
            short,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=500)
        )]
        limit: u32,
        /// Optional opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Start of time window (RFC3339)
        #[arg(long)]
        time_min: Option<String>,
        /// End of time window (RFC3339)
        #[arg(long)]
        time_max: Option<String>,
        /// Connector response format for ingestion/display
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Response format (concise or detailed) for raw output
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Fetch a single event
    #[command(name = "get-event", alias = "get")]
    GetEvent {
        /// Preferred canonical reference (`caldav:event:<base64url>`)
        #[arg(long)]
        item_ref: Option<String>,
        /// Event URL
        #[arg(long)]
        url: Option<String>,
        /// Alias for --url
        #[arg(long)]
        event_url: Option<String>,
        /// Connector response format for ingestion/display
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Response format (concise or detailed) for raw output
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Create a new calendar event
    #[command(name = "create-event", alias = "create")]
    CreateEvent {
        /// Optional calendar URL override
        #[arg(long)]
        calendar_url: Option<String>,
        /// Optional event resource path/name
        #[arg(long)]
        event_path: Option<String>,
        /// Optional full event URL
        #[arg(long)]
        url: Option<String>,
        /// Alias for --url
        #[arg(long)]
        event_url: Option<String>,
        /// Optional event UID
        #[arg(long)]
        uid: Option<String>,
        /// Event title
        #[arg(long)]
        summary: Option<String>,
        /// Event description
        #[arg(long)]
        description: Option<String>,
        /// Event location
        #[arg(long)]
        location: Option<String>,
        /// Event status (CONFIRMED, TENTATIVE, CANCELLED)
        #[arg(long)]
        status: Option<String>,
        /// Organizer email or mailto:
        #[arg(long)]
        organizer: Option<String>,
        /// Start time (RFC3339 or iCal format)
        #[arg(long)]
        start: Option<String>,
        /// End time (RFC3339 or iCal format)
        #[arg(long)]
        end: Option<String>,
        /// Raw VCALENDAR payload (alternative to structured fields)
        #[arg(long)]
        raw_ical: Option<String>,
        /// Connector response format for ingestion/display
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Response format (concise or detailed) for raw output
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Update an existing calendar event
    #[command(name = "update-event", alias = "update")]
    UpdateEvent {
        /// Preferred canonical reference (`caldav:event:<base64url>`)
        #[arg(long)]
        item_ref: Option<String>,
        /// Event URL
        #[arg(long)]
        url: Option<String>,
        /// Alias for --url
        #[arg(long)]
        event_url: Option<String>,
        /// Optional ETag precondition
        #[arg(long)]
        if_match: Option<String>,
        /// Optional event UID
        #[arg(long)]
        uid: Option<String>,
        /// Event title
        #[arg(long)]
        summary: Option<String>,
        /// Event description
        #[arg(long)]
        description: Option<String>,
        /// Event location
        #[arg(long)]
        location: Option<String>,
        /// Event status
        #[arg(long)]
        status: Option<String>,
        /// Organizer email or mailto:
        #[arg(long)]
        organizer: Option<String>,
        /// Start time (RFC3339 or iCal format)
        #[arg(long)]
        start: Option<String>,
        /// End time (RFC3339 or iCal format)
        #[arg(long)]
        end: Option<String>,
        /// Raw VCALENDAR payload (replaces existing event payload)
        #[arg(long)]
        raw_ical: Option<String>,
        /// Connector response format for ingestion/display
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Response format (concise or detailed) for raw output
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Delete an event
    #[command(name = "delete-event", alias = "delete")]
    DeleteEvent {
        /// Preferred canonical reference (`caldav:event:<base64url>`)
        #[arg(long)]
        item_ref: Option<String>,
        /// Event URL
        #[arg(long)]
        url: Option<String>,
        /// Alias for --url
        #[arg(long)]
        event_url: Option<String>,
        /// Optional ETag precondition
        #[arg(long)]
        if_match: Option<String>,
    },
}

/// Google Drive tools
#[derive(Subcommand, Clone)]
pub enum GoogleDriveTools {
    /// List files in Drive
    #[command(name = "list-files", alias = "list")]
    ListFiles {
        /// Drive query string
        #[arg(long, short)]
        q: Option<String>,
        /// Page size per request (1-100)
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=100)
        )]
        page_size: u32,
        /// Total number of files to return (1-5000). Defaults to page_size if omitted.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=5000))]
        limit: Option<u32>,
        /// Optional cursor from a previous response (nextPageToken)
        #[arg(long)]
        page_token: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get file metadata by ID
    #[command(name = "get-file", alias = "get")]
    GetFile {
        /// File ID
        #[arg(long, short)]
        file_id: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Download file content by ID
    #[command(name = "download-file", alias = "download")]
    DownloadFile {
        /// File ID
        #[arg(long, short)]
        file_id: String,
        /// Maximum bytes to download
        #[arg(long)]
        max_bytes: Option<u64>,
    },

    /// Export Google Docs/Sheets/Slides to target MIME type
    #[command(name = "export-file", alias = "export")]
    ExportFile {
        /// File ID
        #[arg(long, short)]
        file_id: String,
        /// Target MIME type (e.g., application/pdf, text/csv)
        #[arg(long, short)]
        mime_type: String,
    },

    /// Upload a small file via base64
    #[command(name = "upload-file", alias = "upload")]
    UploadFile {
        /// File name
        #[arg(long, short)]
        name: String,
        /// MIME type
        #[arg(long, short)]
        mime_type: String,
        /// Base64 encoded data
        #[arg(long, short)]
        data_base64: String,
        /// Parent folder IDs (comma-separated)
        #[arg(long)]
        parents: Option<String>,
    },

    /// Resumable upload via temp file
    #[command(name = "upload-file-resumable", alias = "upload-resumable")]
    UploadFileResumable {
        /// File name
        #[arg(long, short)]
        name: String,
        /// MIME type
        #[arg(long, short)]
        mime_type: String,
        /// Base64 encoded data
        #[arg(long, short)]
        data_base64: String,
        /// Parent folder IDs (comma-separated)
        #[arg(long)]
        parents: Option<String>,
    },
}

/// Google Gmail tools
#[derive(Subcommand, Clone)]
pub enum GoogleGmailTools {
    /// List messages in mailbox
    #[command(name = "list-messages", alias = "list")]
    ListMessages {
        /// Gmail query string
        #[arg(long, short)]
        q: Option<String>,
        /// Total number of results to return (1-5000). Connector paginates internally.
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        max_results: u32,
        /// Optional cursor from a previous response (nextPageToken)
        #[arg(long)]
        page_token: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Decode a Gmail raw message
    #[command(name = "decode-message-raw", alias = "decode")]
    DecodeMessageRaw {
        /// Base64url encoded raw message
        #[arg(long, short)]
        raw_base64url: String,
    },

    /// Get a message by ID
    #[command(name = "get-message", alias = "get")]
    GetMessage {
        /// Message ID
        #[arg(long, short)]
        id: String,
        /// Format (raw, full, metadata)
        #[arg(long, short, default_value = "full")]
        format: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get a thread by ID
    #[command(name = "get-thread", alias = "thread")]
    GetThread {
        /// Thread ID
        #[arg(long, short)]
        id: String,
    },
}

/// Google People tools
#[derive(Subcommand, Clone)]
pub enum GooglePeopleTools {
    /// List contacts
    #[command(name = "list-connections", alias = "list")]
    ListConnections {
        /// Page size per request (1-200)
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=200)
        )]
        page_size: u32,
        /// Total number of contacts to return (1-5000). Defaults to page_size if omitted.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=5000))]
        limit: Option<u32>,
        /// Optional cursor from a previous response (nextPageToken)
        #[arg(long)]
        page_token: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get a person by resourceName
    #[command(name = "get-person", alias = "get")]
    GetPerson {
        /// Resource name (e.g., people/c123)
        #[arg(long, short)]
        resource_name: String,
        /// Comma-separated person fields (e.g., names,emailAddresses,phoneNumbers)
        #[arg(long, short)]
        person_fields: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// Google Search Console tools
#[derive(Subcommand, Clone)]
pub enum GoogleSearchConsoleTools {
    /// List Search Console properties
    #[command(name = "list-sites", alias = "list")]
    ListSites {
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get details for a Search Console property
    #[command(name = "get-site", alias = "site")]
    GetSite {
        /// Property URL (e.g., <https://example.com/> or sc-domain:example.com)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Run a Search Analytics query (clicks, impressions, CTR, position)
    #[command(name = "search-analytics", alias = "analytics")]
    SearchAnalytics {
        /// Property URL (e.g., <https://example.com/> or sc-domain:example.com)
        #[arg(long, short)]
        site_url: String,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start_date: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end_date: String,
        /// Dimensions (comma-separated), e.g. query,page,device,country
        #[arg(long)]
        dimensions: Option<String>,
        /// Row limit (1-25000)
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..=25000))]
        row_limit: u32,
        /// Start row (offset)
        #[arg(long, default_value_t = 0)]
        start_row: u32,
        /// Aggregation type (auto, byProperty, byPage)
        #[arg(long, default_value = "auto")]
        aggregation_type: String,
        /// Search type (web, image, video, news, discover, googleNews)
        #[arg(long)]
        r#type: Option<String>,
        /// Data state (final, all, hourly_all)
        #[arg(long, default_value = "final")]
        data_state: String,
        /// Dimension filter groups as raw JSON (advanced)
        #[arg(long)]
        dimension_filter_groups: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// List sitemaps for a property
    #[command(name = "list-sitemaps", alias = "sitemaps")]
    ListSitemaps {
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get details for a specific sitemap
    #[command(name = "get-sitemap", alias = "sitemap")]
    GetSitemap {
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// Sitemap URL (feedpath in the Search Console API)
        #[arg(long)]
        feedpath: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Submit a sitemap for a property
    #[command(name = "submit-sitemap", alias = "add-sitemap")]
    SubmitSitemap {
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// Sitemap URL (feedpath in the Search Console API)
        #[arg(long)]
        feedpath: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Delete a sitemap for a property
    #[command(name = "delete-sitemap", alias = "rm-sitemap")]
    DeleteSitemap {
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// Sitemap URL (feedpath in the Search Console API)
        #[arg(long)]
        feedpath: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Inspect a URL for indexing status (URL Inspection API)
    #[command(name = "inspect-url", alias = "inspect")]
    InspectUrl {
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// URL to inspect
        #[arg(long)]
        inspection_url: String,
        /// Language code (IETF BCP 47), e.g. en-US
        #[arg(long)]
        language_code: Option<String>,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Build common Search Analytics queries (returns args for search-analytics)
    #[command(name = "query-builder", alias = "qb")]
    QueryBuilder {
        /// Query type (preset)
        #[arg(long)]
        query_type: String,
        /// Property URL
        #[arg(long, short)]
        site_url: String,
        /// Lookback window in days (ending today UTC)
        #[arg(long, default_value_t = 28)]
        days: u32,
        /// Optional filter value (depends on query_type)
        #[arg(long)]
        filter: Option<String>,
    },
}

/// Bing Webmaster Tools tools
#[derive(Subcommand, Clone)]
pub enum BingWebmasterToolsTools {
    /// List sites available in Bing Webmaster Tools
    #[command(name = "list-sites", alias = "list")]
    ListSites {
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get rank and traffic stats (daily updated)
    #[command(name = "get-rank-and-traffic-stats", alias = "rank-traffic")]
    GetRankAndTrafficStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get crawl statistics (Bingbot crawl activity)
    #[command(name = "get-crawl-stats", alias = "crawl-stats")]
    GetCrawlStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get crawl issues (crawl problems)
    #[command(name = "get-crawl-issues", alias = "crawl-issues")]
    GetCrawlIssues {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get keyword research data
    #[command(name = "get-keyword-data", alias = "keyword-data")]
    GetKeywordData {
        /// Keyword or phrase to research
        #[arg(long, short)]
        query: String,
        /// Country code (default: us)
        #[arg(long, default_value = "us")]
        country: String,
        /// Language code (default: en)
        #[arg(long, default_value = "en")]
        language: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get backlink/link count data
    #[command(name = "get-backlinks", alias = "backlinks")]
    GetBacklinks {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Page number for pagination (default: 0)
        #[arg(long, default_value_t = 0)]
        page: u32,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get query stats (weekly updated)
    #[command(name = "get-query-stats", alias = "queries")]
    GetQueryStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get traffic stats for a specific query (daily updated)
    #[command(name = "get-query-traffic-stats", alias = "query-traffic")]
    GetQueryTrafficStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Query string
        #[arg(long, short)]
        query: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get page stats
    #[command(name = "get-page-stats", alias = "pages")]
    GetPageStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get URL submission quota
    #[command(name = "get-url-submission-quota", alias = "quota")]
    GetUrlSubmissionQuota {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Submit a single URL for indexing
    #[command(name = "submit-url", alias = "submit")]
    SubmitUrl {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// URL to submit
        #[arg(long)]
        url: String,
    },

    /// Submit a batch of URLs for indexing
    #[command(name = "submit-url-batch", alias = "submit-batch")]
    SubmitUrlBatch {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Comma-separated URLs to submit
        #[arg(long)]
        url_list: String,
    },

    /// Submit a URL via IndexNow (fast indexing)
    #[command(name = "indexnow-submit-url", alias = "indexnow-submit")]
    IndexNowSubmitUrl {
        /// URL to submit
        #[arg(long)]
        url: String,
        /// Optional host override (derived from url if omitted)
        #[arg(long)]
        host: Option<String>,
        /// Optional key location override (defaults to `https://<host>/<key>.txt`)
        #[arg(long)]
        key_location: Option<String>,
    },

    /// Submit multiple URLs via IndexNow (batch)
    #[command(name = "indexnow-submit-url-batch", alias = "indexnow-submit-batch")]
    IndexNowSubmitUrlBatch {
        /// Comma-separated URLs to submit
        #[arg(long)]
        url_list: String,
        /// Optional host override (derived from urls if omitted)
        #[arg(long)]
        host: Option<String>,
        /// Optional key location override (defaults to `https://<host>/<key>.txt`)
        #[arg(long)]
        key_location: Option<String>,
    },

    /// Get details for a specific URL
    #[command(name = "get-url-info", alias = "url-info")]
    GetUrlInfo {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// URL to fetch info for
        #[arg(long)]
        url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get deep links (sitelinks) for a site
    #[command(name = "get-deep-links", alias = "deep-links")]
    GetDeepLinks {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get URLs blocked by robots.txt
    #[command(name = "get-blocked-urls", alias = "blocked-urls")]
    GetBlockedUrls {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get combined query+page stats
    #[command(name = "get-query-page-stats", alias = "query-page")]
    GetQueryPageStats {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Add a site to Bing Webmaster Tools
    #[command(name = "add-site", alias = "add")]
    AddSite {
        /// Site URL to add
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get site verification status
    #[command(name = "verify-site", alias = "verify")]
    VerifySite {
        /// Site URL
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get content-related SEO issues
    #[command(name = "get-content-issues", alias = "content-issues")]
    GetContentIssues {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get malware/security issues
    #[command(name = "get-malware-issues", alias = "malware-issues")]
    GetMalwareIssues {
        /// Site URL (verified property)
        #[arg(long, short)]
        site_url: String,
        /// Response format (concise or detailed)
        #[arg(long, default_value = "concise")]
        response_format: String,
    },
}

/// LinkedIn tools
#[derive(Subcommand, Clone)]
pub enum LinkedinTools {
    /// Show LinkedIn auth status and derived capabilities
    #[command(name = "auth-status", alias = "status")]
    AuthStatus,

    /// Resolve the authenticated member identity from userinfo or id_token
    #[command(name = "me")]
    Me,

    /// Create a member-authored LinkedIn post
    #[command(name = "share", alias = "post")]
    Share {
        /// Post text/commentary
        #[arg(long)]
        text: String,
        /// Visibility (PUBLIC, CONNECTIONS, LOGGED_IN)
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
        /// Optional article URL
        #[arg(long)]
        url: Option<String>,
        /// Optional LinkedIn media URN (existing uploaded image/video/document)
        #[arg(long)]
        image: Option<String>,
        /// Optional title for URL or media content
        #[arg(long)]
        title: Option<String>,
        /// Optional description for URL or media content
        #[arg(long)]
        description: Option<String>,
        /// Optional author override (member URN or subject id)
        #[arg(long)]
        author: Option<String>,
    },

    /// Create an organization-authored LinkedIn post
    #[command(name = "company-share", alias = "company-post")]
    CompanyShare {
        /// Organization URN or numeric organization id
        #[arg(long)]
        organization: Option<String>,
        /// Post text/commentary
        #[arg(long)]
        text: String,
        /// Visibility (PUBLIC, CONNECTIONS, LOGGED_IN)
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
        /// Optional article URL
        #[arg(long)]
        url: Option<String>,
        /// Optional LinkedIn media URN (existing uploaded image/video/document)
        #[arg(long)]
        image: Option<String>,
        /// Optional title for URL or media content
        #[arg(long)]
        title: Option<String>,
        /// Optional description for URL or media content
        #[arg(long)]
        description: Option<String>,
    },

    /// Make a raw authenticated LinkedIn API request
    #[command(name = "api-request", alias = "request")]
    ApiRequest {
        /// HTTP method
        #[arg(long)]
        method: String,
        /// Relative LinkedIn API path or absolute URL
        #[arg(long)]
        path: String,
        /// Optional JSON object encoded as a string
        #[arg(long)]
        query_json: Option<String>,
        /// Optional JSON object encoded as a string
        #[arg(long)]
        headers_json: Option<String>,
        /// Optional JSON body encoded as a string
        #[arg(long)]
        body_json: Option<String>,
        /// Optional raw scalar body string
        #[arg(long)]
        body: Option<String>,
        /// Override the default LinkedIn REST API version (YYYYMM)
        #[arg(long)]
        linkedin_version: Option<String>,
        /// Force LinkedIn REST headers even for non-/rest paths
        #[arg(long, default_value_t = false)]
        include_linkedin_rest_headers: bool,
    },

    /// Refresh the current LinkedIn access token
    #[command(name = "refresh-token", alias = "refresh")]
    RefreshToken,
}

/// Google Scholar tools
#[derive(Subcommand, Clone)]
pub enum GoogleScholarTools {
    /// Search for papers on Google Scholar
    #[command(name = "search-papers", alias = "search")]
    SearchPapers {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
    },
}

// ============================================================================
// Productivity connector tools
// ============================================================================

/// Atlassian tools (Jira + Confluence)
#[derive(Subcommand, Clone)]
pub enum AtlassianTools {
    /// Test authentication
    #[command(name = "test-auth")]
    TestAuth,

    /// Search Jira issues with JQL
    #[command(name = "jira-search", alias = "jira")]
    JiraSearch {
        /// JQL query
        #[arg(long, short)]
        jql: String,
        /// Starting index
        #[arg(long, default_value_t = 0)]
        start_at: u32,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        max_results: u32,
        /// Comma-separated list of fields to return
        #[arg(long, short)]
        fields: Option<String>,
    },

    /// Get a Jira issue by key
    #[command(name = "jira-get", alias = "issue")]
    JiraGet {
        /// Issue key (e.g., PROJ-123)
        #[arg(long, short)]
        key: String,
        /// Expand options (comma-separated)
        #[arg(long, short)]
        expand: Option<String>,
    },

    /// Search Confluence pages with CQL
    #[command(name = "conf-search", alias = "confluence")]
    ConfSearch {
        /// CQL query
        #[arg(long, short)]
        cql: String,
        /// Starting index
        #[arg(long, default_value_t = 0)]
        start: u32,
        /// Maximum results
        #[arg(long, short, default_value_t = 25)]
        limit: u32,
    },

    /// Get a Confluence page by ID
    #[command(name = "conf-get", alias = "page")]
    ConfGet {
        /// Page ID
        #[arg(long, short)]
        id: String,
        /// Expand options (comma-separated)
        #[arg(long, short)]
        expand: Option<String>,
    },
}

/// Microsoft Graph tools (Microsoft 365)
#[derive(Subcommand, Clone)]
pub enum MicrosoftGraphTools {
    /// List recent Outlook messages
    #[command(name = "list-messages", alias = "messages")]
    ListMessages {
        /// Total messages to return (1-5000). Connector paginates internally.
        #[arg(
            long,
            short,
            default_value_t = 20,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        top: u32,
        /// Optional cursor from a previous response (@odata.nextLink)
        #[arg(long)]
        next_link: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// List upcoming calendar events
    #[command(name = "list-events", alias = "events")]
    ListEvents {
        /// Window in days
        #[arg(
            long,
            short,
            default_value_t = 7,
            value_parser = clap::value_parser!(u32).range(1..=30)
        )]
        days_ahead: u32,
        /// Total events to return (1-5000). Connector paginates internally.
        #[arg(
            long,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Optional cursor from a previous response (@odata.nextLink)
        #[arg(long)]
        next_link: Option<String>,
        /// Response format: concise or detailed
        #[arg(long, default_value = "concise")]
        response_format: String,
    },

    /// Get a message by ID
    #[command(name = "get-message", alias = "message")]
    GetMessage {
        /// Message ID
        #[arg(long, short)]
        message_id: String,
    },

    /// Send a simple email
    #[command(name = "send-mail", alias = "send")]
    SendMail {
        /// Recipient email addresses (comma-separated)
        #[arg(long, short)]
        to: String,
        /// Email subject
        #[arg(long, short)]
        subject: String,
        /// Email body text
        #[arg(long, short)]
        body: String,
    },

    /// Create a draft message
    #[command(name = "create-draft", alias = "draft")]
    CreateDraft {
        /// Recipient email addresses (comma-separated)
        #[arg(long, short)]
        to: String,
        /// Email subject
        #[arg(long, short)]
        subject: String,
        /// Email body text
        #[arg(long, short)]
        body: String,
    },

    /// Upload a large attachment to a draft
    #[command(name = "upload-attachment")]
    UploadAttachment {
        /// Message ID
        #[arg(long, short)]
        message_id: String,
        /// Filename
        #[arg(long, short)]
        filename: String,
        /// MIME type
        #[arg(long, short)]
        mime_type: String,
        /// Base64-encoded data
        #[arg(long, short)]
        data_base64: String,
    },

    /// Send a draft message
    #[command(name = "send-draft")]
    SendDraft {
        /// Message ID
        #[arg(long, short)]
        message_id: String,
    },

    /// Upload attachment from file path
    #[command(name = "upload-attachment-from-path", alias = "upload-file")]
    UploadAttachmentFromPath {
        /// Message ID
        #[arg(long, short)]
        message_id: String,
        /// File path
        #[arg(long, short)]
        file_path: String,
        /// Filename (optional, inferred from path if not provided)
        #[arg(long, short)]
        filename: Option<String>,
        /// MIME type (optional, inferred if not provided)
        #[arg(long, short)]
        mime_type: Option<String>,
    },

    /// Start device authorization flow
    #[command(name = "auth-start")]
    AuthStart {
        /// Tenant ID
        #[arg(long)]
        tenant_id: Option<String>,
        /// Client ID
        #[arg(long)]
        client_id: Option<String>,
        /// Scopes (space-separated)
        #[arg(long)]
        scopes: Option<String>,
    },

    /// Poll token endpoint for device flow
    #[command(name = "auth-poll")]
    AuthPoll {
        /// Tenant ID
        #[arg(long)]
        tenant_id: Option<String>,
        /// Client ID
        #[arg(long, short)]
        client_id: String,
        /// Device code
        #[arg(long, short)]
        device_code: String,
    },
}

/// IMAP email tools
#[derive(Subcommand, Clone)]
pub enum ImapTools {
    /// List mailboxes on the IMAP server
    #[command(name = "list-mailboxes", alias = "mailboxes")]
    ListMailboxes {
        /// IMAP reference name
        #[arg(long, short)]
        reference: Option<String>,
        /// Mailbox pattern (e.g., *)
        #[arg(long, short, default_value = "*")]
        pattern: String,
        /// Include subscription information
        #[arg(long)]
        include_subscribed: bool,
    },

    /// Fetch recent message summaries
    #[command(name = "fetch-messages", alias = "messages")]
    FetchMessages {
        /// Mailbox name
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Total number of messages to return (1-5000). Connector paginates internally.
        #[arg(
            long,
            short,
            default_value_t = 20,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Skip this many messages from the end (offset-based pagination)
        #[arg(long)]
        offset: Option<u32>,
        /// Only fetch messages with UID less than this (cursor-based pagination)
        #[arg(long)]
        before_uid: Option<u32>,
    },

    /// Get a full message by UID
    #[command(name = "get-message", alias = "message")]
    GetMessage {
        /// Mailbox name
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UID
        #[arg(long, short)]
        uid: u32,
        /// Include email headers
        #[arg(long)]
        include_headers: bool,
        /// Include original HTML body
        #[arg(long)]
        include_html: bool,
        /// Include base64 encoded raw message
        #[arg(long)]
        include_raw: bool,
    },

    /// Search messages in a mailbox
    #[command(name = "search")]
    Search {
        /// Mailbox to search
        #[arg(long, short)]
        mailbox: Option<String>,
        /// IMAP search query (e.g., 'UNSEEN', 'FROM "alice"')
        #[arg(long, short)]
        query: String,
        /// Maximum number of UIDs to return
        #[arg(
            long,
            short,
            default_value_t = 50,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Move messages (by UID) from one mailbox to another
    #[command(name = "move-messages", alias = "move")]
    MoveMessages {
        /// Source mailbox name
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Destination mailbox name
        #[arg(long, short = 'd')]
        destination_mailbox: String,
        /// Message UIDs to move (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
        /// Allow expunging all \\Deleted messages if UIDPLUS isn't available (dangerous)
        #[arg(long, default_value_t = false)]
        allow_expunge_all: bool,
    },

    /// Delete messages (by UID) by setting \\Deleted; optionally expunge
    #[command(name = "delete-messages", alias = "delete")]
    DeleteMessages {
        /// Mailbox containing the messages
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UIDs to delete (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Permanently remove messages after marking \\Deleted
        #[arg(long, default_value_t = false)]
        expunge: bool,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
        /// Allow expunging all \\Deleted messages if UIDPLUS isn't available (dangerous)
        #[arg(long, default_value_t = false)]
        allow_expunge_all: bool,
    },

    /// Add flags (e.g. \\Seen, \\Flagged) to messages by UID
    #[command(name = "add-flags")]
    AddFlags {
        /// Mailbox containing the messages
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UIDs to update (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Flags to add (comma-separated), e.g. \\Seen,\\Flagged
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        flags: Vec<String>,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },

    /// Remove flags (e.g. \\Seen, \\Flagged) from messages by UID
    #[command(name = "remove-flags")]
    RemoveFlags {
        /// Mailbox containing the messages
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UIDs to update (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Flags to remove (comma-separated), e.g. \\Seen,\\Flagged
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        flags: Vec<String>,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },

    /// Convenience: mark messages as seen (adds \\Seen)
    #[command(name = "mark-seen")]
    MarkSeen {
        /// Mailbox containing the messages
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UIDs to update (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },

    /// Convenience: mark messages as unseen (removes \\Seen)
    #[command(name = "mark-unseen")]
    MarkUnseen {
        /// Mailbox containing the messages
        #[arg(long, short)]
        mailbox: Option<String>,
        /// Message UIDs to update (comma-separated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        uids: Vec<u32>,
        /// Apply changes (otherwise runs in dry-run mode)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },
}

/// SMTP outbound email tools
#[derive(Subcommand, Clone)]
pub enum SmtpTools {
    /// Send an email using the configured SMTP account
    #[command(name = "send-mail", alias = "send")]
    SendMail {
        /// Recipient email(s), comma-separated for multiple
        #[arg(long, short = 't')]
        to: String,
        /// Subject line
        #[arg(long, short)]
        subject: String,
        /// Plain text body
        #[arg(long, short)]
        body: String,
        /// Optional HTML body (sent as multipart/alternative)
        #[arg(long)]
        html_body: Option<String>,
        /// Optional From override, e.g. "Name <sender@example.com>"
        #[arg(long)]
        from: Option<String>,
        /// Optional Reply-To address
        #[arg(long)]
        reply_to: Option<String>,
        /// Optional CC recipient(s), comma-separated
        #[arg(long)]
        cc: Option<String>,
        /// Optional BCC recipient(s), comma-separated
        #[arg(long)]
        bcc: Option<String>,
        /// Build and validate message only; do not send
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Test SMTP connectivity and authentication with NOOP
    #[command(name = "test-connection", alias = "test")]
    TestConnection,
}

/// Local filesystem tools for text extraction from documents
#[derive(Subcommand, Clone)]
pub enum LocalfsTools {
    /// List files in a directory
    #[command(name = "list-files", alias = "ls")]
    ListFiles {
        /// Directory path to list
        #[arg(long, short)]
        path: String,
        /// Recurse into subdirectories
        #[arg(long, short, default_value_t = false)]
        recursive: bool,
        /// Comma-separated list of extensions to filter (e.g., "pdf,md,txt")
        #[arg(long, short)]
        extensions: Option<String>,
        /// Maximum number of files to return
        #[arg(long, short, default_value_t = 100)]
        limit: u32,
    },

    /// Get metadata about a file
    #[command(name = "file-info", alias = "info")]
    FileInfo {
        /// File path
        #[arg(long, short)]
        path: String,
    },

    /// Extract all text from a file (PDF, EPUB, DOCX, HTML, Markdown, code, text)
    #[command(name = "extract-text", alias = "extract", alias = "read")]
    ExtractText {
        /// File path
        #[arg(long, short)]
        path: String,
        /// Output format: plain or markdown
        #[arg(long, short, default_value = "plain")]
        format: String,
        /// Max characters to return (truncate)
        #[arg(long)]
        max_chars: Option<u32>,
    },

    /// Get document structure (table of contents, headings, chapters)
    #[command(name = "structure", alias = "toc")]
    Structure {
        /// File path
        #[arg(long, short)]
        path: String,
    },

    /// Get a specific section from a document
    #[command(name = "section", alias = "get-section")]
    Section {
        /// File path
        #[arg(long, short)]
        path: String,
        /// Section identifier (e.g., "page:5", "chapter:3", "heading:2", "lines:10-50")
        #[arg(long, short)]
        section: String,
        /// Max characters to return (truncate)
        #[arg(long)]
        max_chars: Option<u32>,
    },

    /// Search within a file
    #[command(name = "search", alias = "grep")]
    Search {
        /// File path
        #[arg(long, short)]
        path: String,
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Lines of context around matches
        #[arg(long, short, default_value_t = 2)]
        context: u32,
    },
}

/// YouTube tools
#[derive(Subcommand, Clone)]
pub enum YoutubeTools {
    /// Search for videos
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
    },

    /// List recent uploads from a channel or playlist
    #[command(name = "list", alias = "recent")]
    List {
        /// Channel ID/URL/handle (e.g., UC..., <https://youtube.com/@hubermanlab>, @hubermanlab)
        #[arg(
            long,
            conflicts_with = "playlist",
            required_unless_present = "playlist"
        )]
        channel: Option<String>,
        /// Playlist ID/URL (e.g., PL..., <https://youtube.com/playlist?list=PL>...)
        #[arg(long, conflicts_with = "channel", required_unless_present = "channel")]
        playlist: Option<String>,
        /// Maximum number of videos to return. Omit to paginate until YouTube stops returning videos.
        #[arg(long, short)]
        limit: Option<u32>,
        /// Only include videos from the last N days (UTC)
        #[arg(long)]
        within_days: Option<u32>,
        /// Only include videos published at/after this RFC3339 timestamp
        #[arg(long)]
        published_after: Option<String>,
    },

    /// Resolve a channel name/handle to a stable UC... channel ID (and ranked candidates for "official" selection)
    #[command(name = "resolve-channel", alias = "resolve", alias = "channel")]
    ResolveChannel {
        /// Channel name query (e.g., "Andrew Huberman")
        #[arg(long)]
        query: Option<String>,
        /// Channel ID/URL/handle to normalize (e.g., "@hubermanlab")
        #[arg(long)]
        channel: Option<String>,
        /// Max candidates to return
        #[arg(long, default_value_t = 5)]
        limit: u32,
        /// Prefer verified channels when ranking candidates
        #[arg(long, default_value_t = true)]
        prefer_verified: bool,
    },

    /// Get video details or enumerate a playlist/channel URL
    #[command(
        name = "get",
        alias = "video",
        alias = "details",
        alias = "get_details",
        alias = "get-details",
        alias = "getdetails"
    )]
    Get {
        /// Video ID/URL, playlist ID/URL, or channel handle/URL (positional)
        #[arg(
            value_name = "ID_OR_URL",
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        id_or_url: Option<String>,
        /// Video ID/URL, playlist ID/URL, or channel handle/URL (flag)
        #[arg(long, short, required_unless_present = "id_or_url")]
        id: Option<String>,
    },

    /// Get video transcript (compat alias; use `rzn-tools youtube get`)
    #[command(name = "transcript", alias = "captions", hide = true)]
    Transcript {
        /// Video ID or URL (positional)
        #[arg(
            value_name = "ID_OR_URL",
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        id_or_url: Option<String>,
        /// Video ID or URL (flag)
        #[arg(long, short, required_unless_present = "id_or_url")]
        id: Option<String>,
    },

    /// Get video chapters (compat alias; use `rzn-tools youtube get`)
    #[command(name = "chapters", hide = true)]
    Chapters {
        /// Video ID or URL (positional)
        #[arg(
            value_name = "ID_OR_URL",
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        id_or_url: Option<String>,
        /// Video ID or URL (flag)
        #[arg(long, short, required_unless_present = "id_or_url")]
        id: Option<String>,
    },
}

/// YouTube command args
///
/// Supports both:
/// - `rzn-tools youtube <ID_OR_URL>` (implicit get/list)
/// - `rzn-tools youtube <subcommand> ...`
#[derive(Args, Clone)]
#[command(args_conflicts_with_subcommands = true, arg_required_else_help = true)]
pub struct YoutubeArgs {
    /// YouTube subcommand
    #[command(subcommand)]
    pub command: Option<YoutubeTools>,

    /// Video, playlist, or channel ID/URL (implicit `get`)
    #[arg(value_name = "ID_OR_URL")]
    pub id_or_url: Option<String>,
}

/// Hacker News tools
#[derive(Subcommand, Clone)]
pub enum HackernewsTools {
    /// Search stories
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
    },

    /// Get a thread by ID
    #[command(name = "thread", alias = "story", alias = "get")]
    Story {
        /// Thread ID
        #[arg(long, short)]
        id: u64,
        /// Maximum number of comments to include in compact output
        #[arg(long, default_value_t = 20)]
        max_comments: u32,
        /// Response format: compact, concise, or detailed
        #[arg(long, default_value = "compact", value_parser = ["compact", "concise", "detailed"])]
        response_format: String,
    },

    /// Get top stories
    #[command(name = "top")]
    Top {
        /// Maximum number of results
        #[arg(long, short, default_value_t = 30)]
        limit: u32,
    },

    /// Get new stories
    #[command(name = "new", alias = "latest")]
    New {
        /// Maximum number of results
        #[arg(long, short, default_value_t = 30)]
        limit: u32,
    },

    /// Get best stories
    #[command(name = "best")]
    Best {
        /// Maximum number of results
        #[arg(long, short, default_value_t = 30)]
        limit: u32,
    },

    /// Get comments for a story
    #[command(name = "comments")]
    Comments {
        /// Story ID
        #[arg(long, short)]
        id: u64,
        /// Maximum number of comments
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },
}

/// arXiv tools
#[derive(Subcommand, Clone)]
pub enum ArxivTools {
    /// Search papers
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
        /// Sort by: relevance, lastUpdatedDate, submittedDate
        #[arg(long, default_value = "relevance")]
        sort: String,
    },

    /// Get paper details
    #[command(name = "paper", alias = "get")]
    Paper {
        /// arXiv ID (e.g., 2301.07041)
        #[arg(long, short)]
        id: String,
    },

    /// Get paper PDF URL
    #[command(name = "pdf")]
    Pdf {
        /// arXiv ID
        #[arg(long, short)]
        id: String,
    },
}

/// GitHub tools
#[derive(Subcommand, Clone)]
pub enum GithubTools {
    /// Search repositories
    #[command(name = "search-repos", alias = "repos")]
    SearchRepos {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
    },

    /// Search code
    #[command(name = "search-code", alias = "code")]
    SearchCode {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Repository (owner/repo)
        #[arg(long, short)]
        repo: Option<String>,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
    },

    /// List repository issues
    #[command(name = "issues")]
    Issues {
        /// Repository (owner/repo)
        #[arg(long, short)]
        repo: String,
        /// State: open, closed, all
        #[arg(long, default_value = "open")]
        state: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 30)]
        limit: u32,
    },

    /// List repository pull requests
    #[command(name = "pulls", alias = "prs")]
    Pulls {
        /// Repository (owner/repo)
        #[arg(long, short)]
        repo: String,
        /// State: open, closed, all
        #[arg(long, default_value = "open")]
        state: String,
        /// Maximum number of results
        #[arg(long, short, default_value_t = 30)]
        limit: u32,
    },

    /// Get repository info
    #[command(name = "repo", alias = "get")]
    Repo {
        /// Repository (owner/repo)
        #[arg(long, short)]
        repo: String,
    },
}

/// Reddit tools
#[derive(Subcommand, Clone)]
pub enum RedditTools {
    /// Search posts
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Subreddit to search in
        #[arg(long, short)]
        subreddit: Option<String>,
        /// Sort order: relevance, hot, new, top, comments
        #[arg(long, default_value = "relevance")]
        sort: String,
        /// Time filter: hour, day, week, month, year, all
        #[arg(long, default_value = "all")]
        time: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Get hot posts
    #[command(name = "hot")]
    Hot {
        /// Subreddit
        #[arg(long, short)]
        subreddit: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Opaque pagination cursor from a previous normalized_v1 response
        #[arg(long, visible_alias = "after")]
        cursor: Option<String>,
        /// Connector response format: raw, normalized_v1, display_v1
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Include NSFW/over-18 listings
        #[arg(long, alias = "include-over-18")]
        include_nsfw: bool,
    },

    /// Get new posts
    #[command(name = "new")]
    New {
        /// Subreddit
        #[arg(long, short)]
        subreddit: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Opaque pagination cursor from a previous normalized_v1 response
        #[arg(long, visible_alias = "after")]
        cursor: Option<String>,
        /// Connector response format: raw, normalized_v1, display_v1
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Include NSFW/over-18 listings
        #[arg(long, alias = "include-over-18")]
        include_nsfw: bool,
    },

    /// Get top posts
    #[command(name = "top")]
    Top {
        /// Subreddit
        #[arg(long, short)]
        subreddit: String,
        /// Time filter: hour, day, week, month, year, all
        #[arg(long, short, default_value = "day")]
        time: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 25,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Opaque pagination cursor from a previous normalized_v1 response
        #[arg(long, visible_alias = "after")]
        cursor: Option<String>,
        /// Connector response format: raw, normalized_v1, display_v1
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
        /// Include NSFW/over-18 listings
        #[arg(long, alias = "include-over-18")]
        include_nsfw: bool,
    },

    /// Resolve ordered media URLs for a post
    #[command(name = "media")]
    Media {
        /// Post ID, item_ref, or URL
        #[arg(long, short)]
        id: String,
        /// Include NSFW/over-18 listings
        #[arg(long, alias = "include-over-18")]
        include_nsfw: bool,
    },

    /// Get post details
    #[command(name = "post", alias = "get")]
    Post {
        /// Post ID or URL
        #[arg(long, short)]
        id: String,
        /// Maximum number of comments to fetch (0-5000). Set to 0 to skip comments.
        #[arg(long, default_value_t = 25, value_parser = clap::value_parser!(u32).range(0..=5000))]
        comment_limit: u32,
        /// Comment sort order: best, top, new, controversial, old, qa
        #[arg(long, default_value = "best", value_parser = ["best", "top", "new", "controversial", "old", "qa"])]
        comment_sort: String,
    },

    /// Get user profile metadata (karma + creation time)
    #[command(name = "user", alias = "profile")]
    User {
        /// Username (with or without u/ prefix)
        #[arg(long, short)]
        username: String,
        /// Connector response format: raw, normalized_v1, display_v1
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },
}

/// App Store tools
#[derive(Subcommand, Clone)]
pub enum AppStoreTools {
    /// Search apps by keyword (iTunes Search API)
    #[command(name = "search", alias = "find")]
    Search {
        /// Search keyword(s)
        #[arg(long, short)]
        query: String,
        /// Storefront country code (ISO 2-letter)
        #[arg(long, default_value = "US")]
        country: String,
        /// Maximum results (1-200)
        #[arg(long, default_value_t = 25, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
    },

    /// Lookup app details by App Store track id (adam id)
    #[command(name = "lookup", alias = "get")]
    Lookup {
        /// App Store track id (adam id)
        #[arg(long)]
        track_id: u64,
        /// Storefront country code (ISO 2-letter)
        #[arg(long, default_value = "US")]
        country: String,
    },

    /// Fetch recent customer reviews via the App Store RSS feed (JSON)
    #[command(name = "reviews")]
    Reviews {
        /// App Store track id (adam id)
        #[arg(long)]
        track_id: u64,
    },

    /// Validate connectivity to the iTunes Search API
    #[command(name = "test-auth")]
    TestAuth,
}

/// App Store Connect tools
#[derive(Subcommand, Clone)]
pub enum AppStoreConnectTools {
    /// List apps in your App Store Connect account
    #[command(name = "list-apps")]
    ListApps {
        /// Limit (1-200)
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Filter by app name (`filter[name]`)
        #[arg(long)]
        filter_name: Option<String>,
        /// Filter by bundle id (`filter[bundleId]`)
        #[arg(long)]
        filter_bundle_id: Option<String>,
        /// Filter by SKU (`filter[sku]`)
        #[arg(long)]
        filter_sku: Option<String>,
    },

    /// Get a single app by App Store Connect app id
    #[command(name = "get-app", alias = "app")]
    GetApp {
        /// App Store Connect app id
        #[arg(long)]
        app_id: String,
    },

    /// Create an Analytics report request for an app
    #[command(name = "create-analytics-report-request")]
    CreateAnalyticsReportRequest {
        /// App Store Connect app id
        #[arg(long)]
        app_id: String,
        /// Report access type (ONE_TIME_SNAPSHOT or ONGOING)
        #[arg(long, default_value = "ONE_TIME_SNAPSHOT")]
        access_type: String,
    },

    /// List Analytics reports for a report request id
    #[command(name = "list-analytics-reports")]
    ListAnalyticsReports {
        /// analyticsReportRequest id
        #[arg(long)]
        report_request_id: String,
        /// Limit (1-200)
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Filter by report category (e.g. APP_USAGE)
        #[arg(long)]
        filter_category: Option<String>,
        /// Filter by report name
        #[arg(long)]
        filter_name: Option<String>,
    },

    /// List Analytics report instances for a report id
    #[command(name = "list-analytics-report-instances")]
    ListAnalyticsReportInstances {
        /// analyticsReport id
        #[arg(long)]
        report_id: String,
        /// Limit (1-200)
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Filter by processing date (YYYY-MM-DD)
        #[arg(long)]
        filter_processing_date: Option<String>,
        /// Filter by granularity (DAILY/WEEKLY/MONTHLY)
        #[arg(long)]
        filter_granularity: Option<String>,
    },

    /// List downloadable report segments for an Analytics report instance id
    #[command(name = "list-analytics-report-segments")]
    ListAnalyticsReportSegments {
        /// analyticsReportInstance id
        #[arg(long)]
        instance_id: String,
        /// Limit (1-200)
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
    },

    /// Download a report segment (bounded preview; usually gzip TSV)
    #[command(name = "download-analytics-report-segment")]
    DownloadAnalyticsReportSegment {
        /// Segment URL (preferred)
        #[arg(long)]
        segment_url: Option<String>,
        /// Segment id (alternative to segment_url; resolved via /analyticsReportSegments/{id})
        #[arg(long)]
        segment_id: Option<String>,
        /// Max compressed KB to download (default 1024)
        #[arg(long)]
        max_kb: Option<u64>,
        /// Max uncompressed KB to parse (default 2048)
        #[arg(long)]
        max_uncompressed_kb: Option<u64>,
        /// Max TSV rows to parse (default 200)
        #[arg(long)]
        max_rows: Option<usize>,
        /// Max characters in the text preview (default 6000)
        #[arg(long)]
        max_preview_chars: Option<usize>,
    },

    /// Download a Sales report (bounded preview; gzip TSV)
    #[command(name = "download-sales-report")]
    DownloadSalesReport {
        /// Vendor number (optional if configured)
        #[arg(long)]
        vendor_number: Option<String>,
        /// Report type (e.g. SALES, INSTALLS)
        #[arg(long, default_value = "SALES")]
        report_type: String,
        /// Report sub type (e.g. SUMMARY, DETAILED)
        #[arg(long, default_value = "SUMMARY")]
        report_sub_type: String,
        /// Frequency (DAILY/WEEKLY/MONTHLY/YEARLY)
        #[arg(long, default_value = "MONTHLY")]
        frequency: String,
        /// Report date (YYYY-MM-DD)
        #[arg(long)]
        report_date: Option<String>,
        /// Optional report version
        #[arg(long)]
        version: Option<String>,
        /// Max compressed KB to download (default 1024)
        #[arg(long)]
        max_kb: Option<u64>,
        /// Max uncompressed KB to parse (default 2048)
        #[arg(long)]
        max_uncompressed_kb: Option<u64>,
        /// Max TSV rows to parse (default 200)
        #[arg(long)]
        max_rows: Option<usize>,
        /// Max characters in the text preview (default 6000)
        #[arg(long)]
        max_preview_chars: Option<usize>,
    },

    /// Download a Finance report (bounded preview; gzip TSV)
    #[command(name = "download-finance-report")]
    DownloadFinanceReport {
        /// Vendor number (optional if configured)
        #[arg(long)]
        vendor_number: Option<String>,
        /// Report type (FINANCIAL or FINANCE_DETAIL)
        #[arg(long, default_value = "FINANCIAL")]
        report_type: String,
        /// Report date (YYYY-MM-DD)
        #[arg(long)]
        report_date: String,
        /// Region code (e.g. US)
        #[arg(long)]
        region_code: String,
        /// Max compressed KB to download (default 1024)
        #[arg(long)]
        max_kb: Option<u64>,
        /// Max uncompressed KB to parse (default 2048)
        #[arg(long)]
        max_uncompressed_kb: Option<u64>,
        /// Max TSV rows to parse (default 200)
        #[arg(long)]
        max_rows: Option<usize>,
        /// Max characters in the text preview (default 6000)
        #[arg(long)]
        max_preview_chars: Option<usize>,
    },

    /// Validate JWT signing + API access
    #[command(name = "test-auth")]
    TestAuth,
}

/// Apple Search Ads tools
#[derive(Subcommand, Clone)]
pub enum AppleSearchAdsTools {
    /// List campaigns
    #[command(name = "list-campaigns")]
    ListCampaigns {
        /// Limit (1-200)
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Offset (>= 0)
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },

    /// Get keyword recommendations for an app
    #[command(name = "keyword-recommendations")]
    KeywordRecommendations {
        /// Numeric App Store app id
        #[arg(long)]
        app_id: u64,
        /// Storefront country code(s), e.g. US
        #[arg(long)]
        storefront_countries: String,
    },

    /// Keyword reporting (POST /reports/keywords)
    #[command(name = "report-keywords")]
    ReportKeywords {
        /// JSON request body (stringified)
        #[arg(long)]
        body: String,
    },

    /// Search terms reporting (POST /reports/searchterms)
    #[command(name = "report-search-terms")]
    ReportSearchTerms {
        /// JSON request body (stringified)
        #[arg(long)]
        body: String,
    },

    /// Campaign keyword reporting (POST /reports/campaigns/{campaign_id}/keywords)
    #[command(name = "report-campaign-keywords")]
    ReportCampaignKeywords {
        /// Campaign id
        #[arg(long)]
        campaign_id: String,
        /// JSON request body (stringified)
        #[arg(long)]
        body: String,
    },

    /// Campaign search terms reporting (POST /reports/campaigns/{campaign_id}/searchterms)
    #[command(name = "report-campaign-search-terms")]
    ReportCampaignSearchTerms {
        /// Campaign id
        #[arg(long)]
        campaign_id: String,
        /// JSON request body (stringified)
        #[arg(long)]
        body: String,
    },

    /// Create a campaign (POST /campaigns)
    #[command(name = "create-campaign")]
    CreateCampaign {
        /// JSON request body (stringified)
        #[arg(long)]
        body: String,
    },

    /// Validate OAuth token + API access
    #[command(name = "test-auth")]
    TestAuth,
}

/// Polymarket tools
#[derive(Subcommand, Clone)]
pub enum PolymarketTools {
    /// Browse Polymarket tags for tag-slug discovery
    #[command(name = "list-tags", alias = "tags")]
    ListTags {
        /// Maximum number of tags to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Starting offset
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Browse events with filters such as series or tag
    #[command(name = "list-events", alias = "events")]
    ListEvents {
        /// Maximum number of events to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Starting offset
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Filter to a series id
        #[arg(long)]
        series_id: Option<String>,
        /// Filter to a series slug
        #[arg(long)]
        series_slug: Option<String>,
        /// Filter to a tag slug such as crypto
        #[arg(long)]
        tag_slug: Option<String>,
        /// Only active events
        #[arg(long, default_value_t = false)]
        active: bool,
        /// Only closed events
        #[arg(long, default_value_t = false)]
        closed: bool,
        /// Only archived events
        #[arg(long, default_value_t = false)]
        archived: bool,
        /// Only featured events
        #[arg(long, default_value_t = false)]
        featured: bool,
    },

    /// Browse markets directly or flatten related markets from an event/series/tag
    #[command(name = "list-markets", alias = "markets")]
    ListMarkets {
        /// Maximum number of markets to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Starting offset
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Direct market slug lookup
        #[arg(long)]
        slug: Option<String>,
        /// Event item_ref like polymarket:event:312712
        #[arg(long)]
        event_item_ref: Option<String>,
        /// Event id whose markets should be listed
        #[arg(long)]
        event_id: Option<String>,
        /// Event slug whose markets should be listed
        #[arg(long)]
        event_slug: Option<String>,
        /// Series id to flatten into markets
        #[arg(long)]
        series_id: Option<String>,
        /// Series slug to flatten into markets
        #[arg(long)]
        series_slug: Option<String>,
        /// Tag slug to flatten into markets
        #[arg(long)]
        tag_slug: Option<String>,
        /// Only active markets
        #[arg(long, default_value_t = false)]
        active: bool,
        /// Only closed markets
        #[arg(long, default_value_t = false)]
        closed: bool,
    },

    /// Browse recurring series
    #[command(name = "list-series", alias = "series")]
    ListSeries {
        /// Maximum number of series to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Starting offset
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Exact series slug filter
        #[arg(long)]
        slug: Option<String>,
        /// Only active series
        #[arg(long, default_value_t = false)]
        active: bool,
        /// Only closed series
        #[arg(long, default_value_t = false)]
        closed: bool,
        /// Only featured series
        #[arg(long, default_value_t = false)]
        featured: bool,
    },

    /// Get a single series by id or slug
    #[command(name = "get-series", alias = "series-get")]
    GetSeries {
        /// Series id
        #[arg(long)]
        id: Option<String>,
        /// Series slug
        #[arg(long)]
        slug: Option<String>,
    },

    /// List comments for an event, market, or series
    #[command(name = "list-comments", alias = "comments")]
    ListComments {
        /// Maximum number of comments to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Starting offset
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Event or market item_ref
        #[arg(long)]
        item_ref: Option<String>,
        /// Polymarket event URL
        #[arg(long)]
        event_url: Option<String>,
        /// Event id
        #[arg(long)]
        event_id: Option<String>,
        /// Event slug
        #[arg(long)]
        event_slug: Option<String>,
        /// Market id
        #[arg(long)]
        market_id: Option<String>,
        /// Market slug
        #[arg(long)]
        market_slug: Option<String>,
        /// Series id
        #[arg(long)]
        series_id: Option<String>,
        /// Series slug
        #[arg(long)]
        series_slug: Option<String>,
    },

    /// Inspect top-of-book depth for a market or outcome
    #[command(name = "order-book", alias = "book")]
    OrderBook {
        /// Market item_ref
        #[arg(long)]
        item_ref: Option<String>,
        /// Market id
        #[arg(long)]
        id: Option<String>,
        /// Market slug
        #[arg(long)]
        slug: Option<String>,
        /// Optional outcome name
        #[arg(long)]
        outcome: Option<String>,
        /// Optional direct token id
        #[arg(long)]
        token_id: Option<String>,
        /// Depth per side
        #[arg(long, default_value_t = 5)]
        depth: usize,
    },

    /// Inspect token-level price history wrapped at the market level
    #[command(name = "price-history", alias = "history")]
    PriceHistory {
        /// Market item_ref
        #[arg(long)]
        item_ref: Option<String>,
        /// Market id
        #[arg(long)]
        id: Option<String>,
        /// Market slug
        #[arg(long)]
        slug: Option<String>,
        /// Optional outcome name
        #[arg(long)]
        outcome: Option<String>,
        /// Optional direct token id
        #[arg(long)]
        token_id: Option<String>,
        /// History interval such as 1d or max
        #[arg(long, default_value = "1d")]
        interval: String,
        /// Sampling fidelity in seconds
        #[arg(long, default_value_t = 60)]
        fidelity: u32,
    },

    /// Inspect public holder/position data for a market
    #[command(name = "market-positions", alias = "positions")]
    MarketPositions {
        /// Market item_ref
        #[arg(long)]
        item_ref: Option<String>,
        /// Market id
        #[arg(long)]
        id: Option<String>,
        /// Market slug
        #[arg(long)]
        slug: Option<String>,
        /// Maximum number of positions per token
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },

    /// Fetch one market plus its linked event, order books, history, and optional positions
    #[command(name = "market-context", alias = "context")]
    MarketContext {
        /// Market item_ref
        #[arg(long)]
        item_ref: Option<String>,
        /// Market id
        #[arg(long)]
        id: Option<String>,
        /// Market slug
        #[arg(long)]
        slug: Option<String>,
        /// Book depth per outcome
        #[arg(long, default_value_t = 5)]
        depth: usize,
        /// Embedded price-history interval
        #[arg(long, default_value = "1d")]
        interval: String,
        /// Sampling fidelity in seconds
        #[arg(long, default_value_t = 60)]
        fidelity: u32,
        /// Also fetch public holder data
        #[arg(long, default_value_t = false)]
        include_positions: bool,
        /// Maximum number of public positions per token
        #[arg(long, default_value_t = 20)]
        positions_limit: u32,
    },
}

/// Kalshi tools
#[derive(Subcommand, Clone)]
pub enum KalshiTools {
    /// Search Kalshi series, events, and markets
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long)]
        query: String,
        /// Maximum number of results
        #[arg(long, default_value_t = 10)]
        limit: u32,
        /// Connector response format
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },

    /// Browse Kalshi series
    #[command(name = "list-series", alias = "series")]
    ListSeries {
        /// Maximum number of series to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Optional series status filter
        #[arg(long)]
        status: Option<String>,
    },

    /// Get one series by ticker or item_ref
    #[command(name = "get-series", alias = "series-get")]
    GetSeries {
        /// Normalized item_ref like kalshi:series:KXELONMARS
        #[arg(long)]
        item_ref: Option<String>,
        /// Series ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Number of related events to include
        #[arg(long, default_value_t = 10)]
        events_limit: u32,
        /// Connector response format
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },

    /// Browse Kalshi events
    #[command(name = "list-events", alias = "events")]
    ListEvents {
        /// Maximum number of events to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Filter to one series ticker
        #[arg(long)]
        series_ticker: Option<String>,
        /// Optional event status filter
        #[arg(long)]
        status: Option<String>,
        /// Use the multivariate events endpoint
        #[arg(long, default_value_t = false)]
        multivariate: bool,
        /// Multivariate collection ticker
        #[arg(long)]
        collection_ticker: Option<String>,
    },

    /// Get one event by ticker, item_ref, or URL
    #[command(name = "get-event", alias = "event")]
    GetEvent {
        /// Normalized item_ref like kalshi:event:KXELONMARS-99
        #[arg(long)]
        item_ref: Option<String>,
        /// Event ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Kalshi event URL
        #[arg(long)]
        url: Option<String>,
        /// Connector response format
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },

    /// Get event metadata such as settlement sources
    #[command(name = "event-metadata", alias = "metadata")]
    EventMetadata {
        /// Normalized item_ref like kalshi:event:KXELONMARS-99
        #[arg(long)]
        item_ref: Option<String>,
        /// Event ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Kalshi event URL
        #[arg(long)]
        url: Option<String>,
    },

    /// Get event-level candlesticks
    #[command(name = "event-candles", alias = "event-candlesticks")]
    EventCandles {
        /// Normalized item_ref like kalshi:event:KXELONMARS-99
        #[arg(long)]
        item_ref: Option<String>,
        /// Event ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Kalshi event URL
        #[arg(long)]
        url: Option<String>,
        /// Series ticker override
        #[arg(long)]
        series_ticker: Option<String>,
        /// Unix start timestamp
        #[arg(long)]
        start_ts: i64,
        /// Unix end timestamp
        #[arg(long)]
        end_ts: Option<i64>,
        /// Candle interval in seconds
        #[arg(long, default_value_t = 60)]
        period_interval: u32,
    },

    /// Browse Kalshi markets
    #[command(name = "list-markets", alias = "markets")]
    ListMarkets {
        /// Maximum number of markets to return
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Filter to one series ticker
        #[arg(long)]
        series_ticker: Option<String>,
        /// Filter to one event ticker
        #[arg(long)]
        event_ticker: Option<String>,
        /// Optional market status filter
        #[arg(long)]
        status: Option<String>,
        /// Query the historical markets catalog
        #[arg(long, default_value_t = false)]
        historical: bool,
    },

    /// Get one market by ticker or item_ref
    #[command(name = "get-market", alias = "market")]
    GetMarket {
        /// Normalized item_ref like kalshi:market:KX...
        #[arg(long)]
        item_ref: Option<String>,
        /// Market ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Connector response format
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },

    /// Get the top of book for one market
    #[command(name = "order-book", alias = "book")]
    OrderBook {
        /// Normalized item_ref like kalshi:market:KX...
        #[arg(long)]
        item_ref: Option<String>,
        /// Market ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Maximum number of levels per side
        #[arg(long, default_value_t = 10)]
        depth: usize,
    },

    /// Get market candlesticks
    #[command(name = "market-candles", alias = "candles")]
    MarketCandles {
        /// Normalized item_ref like kalshi:market:KX...
        #[arg(long)]
        item_ref: Option<String>,
        /// Market ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Unix start timestamp
        #[arg(long)]
        start_ts: i64,
        /// Unix end timestamp
        #[arg(long)]
        end_ts: Option<i64>,
        /// Candle interval in seconds
        #[arg(long, default_value_t = 60)]
        period_interval: u32,
    },

    /// List recent trades for one market
    #[command(name = "list-trades", alias = "trades")]
    ListTrades {
        /// Normalized item_ref like kalshi:market:KX...
        #[arg(long)]
        item_ref: Option<String>,
        /// Market ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Maximum number of trades
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Opaque cursor from a previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Minimum unix timestamp filter
        #[arg(long)]
        min_ts: Option<i64>,
        /// Maximum unix timestamp filter
        #[arg(long)]
        max_ts: Option<i64>,
    },

    /// Get bundled market, event, series, trade, and candle context
    #[command(name = "market-context", alias = "context")]
    MarketContext {
        /// Normalized item_ref like kalshi:market:KX...
        #[arg(long)]
        item_ref: Option<String>,
        /// Market ticker
        #[arg(long)]
        ticker: Option<String>,
        /// Unix start timestamp
        #[arg(long)]
        start_ts: Option<i64>,
        /// Unix end timestamp
        #[arg(long)]
        end_ts: Option<i64>,
        /// Candle interval in seconds
        #[arg(long, default_value_t = 60)]
        period_interval: u32,
        /// Maximum number of book levels per side
        #[arg(long, default_value_t = 10)]
        orderbook_depth: usize,
        /// Maximum number of trades to include
        #[arg(long, default_value_t = 20)]
        trades_limit: u32,
        /// Skip bundled event metadata
        #[arg(long, default_value_t = false)]
        skip_event_metadata: bool,
    },
}

/// Play Store tools
#[derive(Subcommand, Clone)]
pub enum PlayStoreTools {
    /// Get app metadata by package id (best-effort)
    #[command(name = "app", alias = "get")]
    App {
        /// Android package id (e.g., com.whatsapp)
        #[arg(long, short)]
        id: String,
        /// UI language hint (best-effort; parsing is most reliable with 'en')
        #[arg(long, default_value = "en")]
        hl: String,
        /// Region hint (2-letter country code)
        #[arg(long, default_value = "US")]
        gl: String,
        /// Connector response format: raw, normalized_v1, display_v1
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },
}

/// Web scraping tools
#[derive(Subcommand, Clone)]
pub enum WebTools {
    /// Scrape a web page
    #[command(name = "scrape", alias = "get")]
    Scrape {
        /// URL to scrape
        #[arg(long, short)]
        url: String,
        /// Output format: text, markdown, html
        #[arg(long, short, default_value = "markdown")]
        format: String,
    },

    /// Extract main content from a page
    #[command(name = "extract")]
    Extract {
        /// URL to extract from
        #[arg(long, short)]
        url: String,
        /// Extract images
        #[arg(long)]
        images: bool,
        /// Extract links
        #[arg(long)]
        links: bool,
    },

    /// Get page metadata
    #[command(name = "metadata", alias = "meta")]
    Metadata {
        /// URL
        #[arg(long, short)]
        url: String,
    },
}

/// Wikipedia tools
#[derive(Subcommand, Clone)]
pub enum WikipediaTools {
    /// Search articles
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Get article content
    #[command(name = "article", alias = "get")]
    Article {
        /// Article title
        #[arg(long, short)]
        title: String,
    },

    /// Get article summary
    #[command(name = "summary")]
    Summary {
        /// Article title
        #[arg(long, short)]
        title: String,
    },
}

/// PubMed tools
#[derive(Subcommand, Clone)]
pub enum PubmedTools {
    /// Search articles
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Get article by PMID
    #[command(name = "article", alias = "get")]
    Article {
        /// PubMed ID
        #[arg(long, short)]
        pmid: String,
    },
}

/// Semantic Scholar tools
#[derive(Subcommand, Clone)]
pub enum SemanticScholarTools {
    /// Search papers
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 10,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Get paper details
    #[command(name = "paper", alias = "get")]
    Paper {
        /// Paper ID
        #[arg(long, short)]
        id: String,
    },

    /// Get paper citations
    #[command(name = "citations")]
    Citations {
        /// Paper ID
        #[arg(long, short)]
        id: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 50,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },

    /// Get paper references
    #[command(name = "references")]
    References {
        /// Paper ID
        #[arg(long, short)]
        id: String,
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 50,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
    },
}

/// Slack tools
#[derive(Subcommand, Clone)]
pub enum SlackTools {
    /// List channels
    #[command(name = "channels", alias = "list-channels")]
    Channels {
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 100,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Get channel messages
    #[command(name = "messages", alias = "history")]
    Messages {
        /// Channel name or ID
        #[arg(long, short)]
        channel: String,
        /// Maximum number of messages
        #[arg(
            long,
            short,
            default_value_t = 50,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Search messages
    #[command(name = "search")]
    Search {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Results per page (1-100)
        #[arg(
            long,
            short,
            default_value_t = 50,
            value_parser = clap::value_parser!(u32).range(1..=100)
        )]
        limit: u32,
        /// Page number (1+)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        /// Sort order: score or timestamp
        #[arg(long, value_parser = ["score", "timestamp"])]
        sort: Option<String>,
        /// Sort direction: asc or desc
        #[arg(long, value_parser = ["asc", "desc"])]
        sort_dir: Option<String>,
    },

    /// List users
    #[command(name = "users")]
    Users {
        /// Maximum number of results
        #[arg(
            long,
            short,
            default_value_t = 100,
            value_parser = clap::value_parser!(u32).range(1..=5000)
        )]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },
}

/// X (Twitter) tools
#[derive(Subcommand, Clone)]
pub enum XTools {
    /// Get user profile
    #[command(name = "profile", alias = "get-profile")]
    Profile {
        /// X username
        #[arg(long, short)]
        username: String,
    },

    /// Search tweets
    #[command(name = "search", alias = "search-tweets")]
    SearchTweets {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of tweets
        #[arg(long, short, value_parser = clap::value_parser!(u32).range(1..=50))]
        limit: Option<u32>,
        /// Pagination cursor (use next_cursor from previous call)
        #[arg(long)]
        cursor: Option<String>,
        /// Search mode: top, latest, photos, videos
        #[arg(long, value_parser = ["top", "latest", "photos", "videos"])]
        mode: Option<String>,
        /// Date filter: YYYY-MM-DD (appended as `since:YYYY-MM-DD` to the query)
        #[arg(long)]
        since: Option<String>,
        /// Date filter: YYYY-MM-DD (appended as `until:YYYY-MM-DD` to the query)
        #[arg(long)]
        until: Option<String>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        start_time: Option<String>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        end_time: Option<String>,
        /// Exclude reply tweets (local post-filter)
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets (local post-filter)
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Minimum likes (local post-filter)
        #[arg(long)]
        min_likes: Option<i64>,
        /// Minimum retweets (local post-filter)
        #[arg(long)]
        min_retweets: Option<i64>,
        /// Minimum replies (local post-filter)
        #[arg(long)]
        min_replies: Option<i64>,
        /// Minimum views (local post-filter)
        #[arg(long)]
        min_views: Option<i64>,
        /// Sort by: time or engagement (local sort)
        #[arg(long, value_parser = ["time", "engagement"])]
        sort_by: Option<String>,
        /// Sort order: asc or desc (local sort)
        #[arg(long, value_parser = ["asc", "desc"])]
        order: Option<String>,
    },

    /// Get user followers
    #[command(name = "followers", alias = "get-followers")]
    Followers {
        /// Username
        #[arg(long, short)]
        username: String,
        /// Maximum number of followers
        #[arg(long, short)]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Get tweet details
    #[command(name = "tweet", alias = "get-tweet")]
    Tweet {
        /// Tweet ID
        #[arg(long, short)]
        tweet_id: String,
    },

    /// Get home timeline
    #[command(name = "timeline", alias = "home")]
    Timeline {
        /// Number of tweets
        #[arg(long, short)]
        count: u32,
        /// Exclude replies
        #[arg(long)]
        exclude_replies: Option<bool>,
    },

    /// Fetch tweets and replies
    #[command(name = "tweets-and-replies")]
    TweetsAndReplies {
        /// Username
        #[arg(long, short)]
        username: String,
        /// Maximum number of tweets
        #[arg(long, short)]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Fetch a user's tweets (no replies)
    #[command(name = "tweets", alias = "user-tweets")]
    UserTweets {
        /// Username (no @) or numeric user_id
        #[arg(long, short)]
        username: String,
        /// Maximum number of tweets to return
        #[arg(long, short, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: Option<u32>,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
        /// Exclude retweets (local post-filter)
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        start_time: Option<String>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        end_time: Option<String>,
        /// Sort order by time: asc or desc
        #[arg(long, value_parser = ["asc", "desc"])]
        order: Option<String>,
    },

    /// Fetch a tweet thread/conversation
    #[command(name = "thread", alias = "get-thread")]
    Thread {
        /// Focal tweet id
        #[arg(long, short)]
        tweet_id: String,
        /// Maximum number of tweets to return from the conversation
        #[arg(long, short, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: Option<u32>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        start_time: Option<String>,
        /// RFC3339 datetime (or YYYY-MM-DD). Applied as a local post-filter.
        #[arg(long)]
        end_time: Option<String>,
        /// Exclude replies (local post-filter)
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets (local post-filter)
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Sort by: time or engagement (local sort)
        #[arg(long, value_parser = ["time", "engagement"])]
        sort_by: Option<String>,
        /// Sort order: asc or desc (local sort)
        #[arg(long, value_parser = ["asc", "desc"])]
        order: Option<String>,
    },

    /// Search profiles
    #[command(name = "search-profiles")]
    SearchProfiles {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of profiles
        #[arg(long, short)]
        limit: u32,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Get direct message conversations
    #[command(name = "dm-conversations", alias = "dms")]
    DmConversations {
        /// User ID
        #[arg(long, short)]
        user_id: String,
        /// Pagination cursor
        #[arg(long)]
        cursor: Option<String>,
    },

    /// Send direct message
    #[command(name = "send-dm")]
    SendDm {
        /// Conversation ID
        #[arg(long, short)]
        conversation_id: String,
        /// Message text
        #[arg(long, short)]
        text: String,
    },
}

/// X (Twitter) API tools
#[derive(Subcommand, Clone)]
pub enum XApiTools {
    /// Show configured X auth families and preferred auth routing
    #[command(name = "auth-status")]
    AuthStatus,

    /// Validate user-context auth against /users/me
    #[command(name = "whoami")]
    WhoAmI {
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Search recent tweets via the official API (cursor = next_token)
    #[command(name = "search")]
    SearchRecentTweets {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Maximum number of tweets (10..100)
        #[arg(long, value_parser = clap::value_parser!(u32).range(10..=100))]
        max_results: Option<u32>,
        /// Number of pages to fetch (1..5)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=5))]
        pages: Option<u32>,
        /// Maximum number of tweets to return (after filtering/sorting)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination token from previous response
        #[arg(long)]
        next_token: Option<String>,
        /// Relative lookback (e.g. 12h, 7d). Ignored if --start-time is set.
        #[arg(long)]
        since: Option<String>,
        /// Start time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        start_time: Option<String>,
        /// End time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        end_time: Option<String>,
        /// Sort order: recency or relevancy
        #[arg(long, value_parser = ["recency", "relevancy"])]
        sort_order: Option<String>,
        /// Client-side sort key for fetched pages
        #[arg(long, value_parser = ["time", "likes", "retweets", "replies", "quotes", "views", "engagement"])]
        sort_by: Option<String>,
        /// Client-side sort direction
        #[arg(long, value_parser = ["asc", "desc"])]
        order: Option<String>,
        /// Exclude replies (adds -is:reply)
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets (adds -is:retweet)
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Filter: minimum likes
        #[arg(long)]
        min_likes: Option<i64>,
        /// Filter: minimum retweets
        #[arg(long)]
        min_retweets: Option<i64>,
        /// Filter: minimum replies
        #[arg(long)]
        min_replies: Option<i64>,
        /// Filter: minimum quotes
        #[arg(long)]
        min_quotes: Option<i64>,
        /// Filter: minimum views (best-effort)
        #[arg(long)]
        min_views: Option<i64>,
        /// Convenience: append from:username if no from: is present
        #[arg(long)]
        from_username: Option<String>,
        /// Quick mode (1 page, <= 10 tweets, exclude replies/retweets)
        #[arg(long)]
        quick: Option<bool>,
        /// Quality mode (defaults min_likes to 10)
        #[arg(long)]
        quality: Option<bool>,
        /// Include raw API pages in the output
        #[arg(long)]
        include_raw: Option<bool>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Get a tweet by id via the official API
    #[command(name = "tweet", alias = "get-tweet")]
    Tweet {
        /// Tweet ID
        #[arg(long, short)]
        tweet_id: String,
        /// tweet.fields override (comma-separated)
        #[arg(long)]
        tweet_fields: Option<String>,
        /// expansions override (comma-separated)
        #[arg(long)]
        expansions: Option<String>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Get a user by username via the official API
    #[command(name = "user", alias = "get-user")]
    UserByUsername {
        /// Username (no @)
        #[arg(long, short)]
        username: String,
        /// user.fields override (comma-separated)
        #[arg(long)]
        user_fields: Option<String>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Get a profile by username via the official API (alias for user)
    #[command(name = "profile")]
    Profile {
        /// Username (no @)
        #[arg(long, short)]
        username: String,
        /// user.fields override (comma-separated)
        #[arg(long)]
        user_fields: Option<String>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Fetch a recent conversation/thread snapshot for a tweet id
    #[command(name = "thread")]
    Thread {
        /// Tweet ID
        #[arg(long, short)]
        tweet_id: String,
        /// Tweets per page (10..100)
        #[arg(long, value_parser = clap::value_parser!(u32).range(10..=100))]
        max_results: Option<u32>,
        /// Number of pages to fetch (1..5)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=5))]
        pages: Option<u32>,
        /// Maximum number of tweets to return (after sorting)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination token from previous response
        #[arg(long)]
        next_token: Option<String>,
        /// Relative lookback (e.g. 12h, 7d). Ignored if --start-time is set.
        #[arg(long)]
        since: Option<String>,
        /// Start time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        start_time: Option<String>,
        /// End time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        end_time: Option<String>,
        /// Exclude replies (best-effort)
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets (adds -is:retweet)
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Sort direction by created_at
        #[arg(long, value_parser = ["asc", "desc"])]
        order: Option<String>,
        /// Include the root tweet object in the output
        #[arg(long)]
        include_root: Option<bool>,
        /// Include raw API pages in the output
        #[arg(long)]
        include_raw: Option<bool>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Fetch a user's tweets via the official API
    #[command(name = "user-tweets")]
    UserTweets {
        /// Numeric user id
        #[arg(long)]
        user_id: String,
        /// Maximum number of tweets (5..100)
        #[arg(long, value_parser = clap::value_parser!(u32).range(5..=100))]
        max_results: Option<u32>,
        /// Pagination token from previous response
        #[arg(long)]
        pagination_token: Option<String>,
        /// Start time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        start_time: Option<String>,
        /// End time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        end_time: Option<String>,
        /// Exclude replies
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Fetch a user's tweets by username (resolves username -> user_id)
    #[command(name = "profile-tweets")]
    ProfileTweets {
        /// Username (no @)
        #[arg(long, short)]
        username: String,
        /// Maximum number of tweets (5..100)
        #[arg(long, value_parser = clap::value_parser!(u32).range(5..=100))]
        max_results: Option<u32>,
        /// Pagination token from previous response
        #[arg(long)]
        pagination_token: Option<String>,
        /// Start time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        start_time: Option<String>,
        /// End time (RFC3339 or YYYY-MM-DD UTC)
        #[arg(long)]
        end_time: Option<String>,
        /// Exclude replies
        #[arg(long)]
        exclude_replies: Option<bool>,
        /// Exclude retweets
        #[arg(long)]
        exclude_retweets: Option<bool>,
        /// Force an auth mode instead of auto selection
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Search full-archive tweets when your X API tier allows it
    #[command(name = "search-all")]
    SearchAllTweets {
        #[arg(long, short)]
        query: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(10..=100))]
        max_results: Option<u32>,
        #[arg(long)]
        next_token: Option<String>,
        #[arg(long)]
        start_time: Option<String>,
        #[arg(long)]
        end_time: Option<String>,
        #[arg(long, value_parser = ["recency", "relevancy"])]
        sort_order: Option<String>,
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Get authenticated-user mentions
    #[command(name = "mentions")]
    Mentions {
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u32).range(5..=100))]
        max_results: Option<u32>,
        #[arg(long)]
        pagination_token: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Get authenticated-user home timeline
    #[command(name = "home")]
    HomeTimeline {
        #[arg(long, value_parser = clap::value_parser!(u32).range(5..=100))]
        max_results: Option<u32>,
        #[arg(long)]
        pagination_token: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Create a post
    #[command(name = "create-post")]
    CreatePost {
        #[arg(long)]
        text: String,
        #[arg(long)]
        reply_to_tweet_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    /// Delete a post
    #[command(name = "delete-post")]
    DeletePost {
        #[arg(long)]
        tweet_id: String,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "like")]
    LikePost {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "unlike")]
    UnlikePost {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "repost")]
    RepostPost {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "unrepost")]
    UnrepostPost {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "follow")]
    FollowUser {
        #[arg(long)]
        target_user_id: String,
        #[arg(long)]
        source_user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "unfollow")]
    UnfollowUser {
        #[arg(long)]
        target_user_id: String,
        #[arg(long)]
        source_user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "bookmarks")]
    GetBookmarks {
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u32).range(5..=100))]
        max_results: Option<u32>,
        #[arg(long)]
        pagination_token: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "add-bookmark")]
    AddBookmark {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "remove-bookmark")]
    RemoveBookmark {
        #[arg(long)]
        tweet_id: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "refresh-oauth2")]
    RefreshOauth2,

    #[command(name = "usage")]
    GetUsage {
        #[arg(long)]
        days: Option<u32>,
        #[arg(long)]
        usage_fields: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "create-list")]
    CreateList {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        private: Option<bool>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "update-list")]
    UpdateList {
        #[arg(long)]
        list_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        private: Option<bool>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "delete-list")]
    DeleteList {
        #[arg(long)]
        list_id: String,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "create-dm-conversation")]
    CreateDmConversation {
        #[arg(long)]
        participant_ids: Vec<String>,
        #[arg(long)]
        text: String,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "dm-events")]
    GetDmEvents {
        #[arg(long)]
        conversation_id: String,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
        max_results: Option<u32>,
        #[arg(long)]
        pagination_token: Option<String>,
        #[arg(long)]
        event_types: Option<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "media-init")]
    InitializeMediaUpload {
        #[arg(long)]
        media_type: String,
        #[arg(long)]
        total_bytes: u64,
        #[arg(long)]
        media_category: Option<String>,
        #[arg(long)]
        shared: Option<bool>,
        #[arg(long)]
        additional_owners: Vec<String>,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "media-append")]
    AppendMediaUpload {
        #[arg(long)]
        upload_id: String,
        #[arg(long)]
        segment_index: u64,
        #[arg(long)]
        media_base64: String,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "media-finalize")]
    FinalizeMediaUpload {
        #[arg(long)]
        upload_id: String,
        #[arg(long, value_parser = ["auto", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },

    #[command(name = "raw-operation")]
    RawOperation {
        #[arg(long)]
        operation_id: String,
        #[arg(long)]
        path_params: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, value_parser = ["auto", "bearer", "oauth2", "oauth1"])]
        auth_mode: Option<String>,
    },
}

/// Discord tools
#[derive(Subcommand, Clone)]
pub enum DiscordTools {
    /// List servers
    #[command(name = "servers", alias = "list-servers")]
    Servers,

    /// Get server info
    #[command(name = "server", alias = "server-info")]
    Server {
        /// Guild/server ID
        #[arg(long, short)]
        guild_id: u64,
    },

    /// List channels
    #[command(name = "channels", alias = "list-channels")]
    Channels {
        /// Guild/server ID
        #[arg(long, short)]
        guild_id: u64,
    },

    /// Read messages
    #[command(name = "messages", alias = "read-messages")]
    Messages {
        /// Channel ID
        #[arg(long, short)]
        channel_id: u64,
        /// Number of messages (max 100)
        #[arg(long, short)]
        limit: Option<u32>,
    },

    /// Send message
    #[command(name = "send", alias = "send-message")]
    Send {
        /// Channel ID
        #[arg(long, short)]
        channel_id: u64,
        /// Message content
        #[arg(long, short)]
        content: String,
    },

    /// Search messages
    #[command(name = "search")]
    Search {
        /// Channel ID
        #[arg(long, short)]
        channel_id: u64,
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Number of matching messages (max 100)
        #[arg(long, short)]
        limit: Option<u32>,
    },
}

/// RSS tools
#[derive(Subcommand, Clone)]
pub enum RssTools {
    /// Get feed metadata and recent entries
    #[command(name = "feed", alias = "get-feed")]
    Feed {
        /// Feed URL
        #[arg(long, short)]
        url: String,
        /// Number of entries
        #[arg(long, short)]
        limit: Option<u32>,
    },

    /// List feed entries
    #[command(name = "entries", alias = "list-entries")]
    Entries {
        /// Feed URL
        #[arg(long, short)]
        url: String,
        /// Number of entries
        #[arg(long, short)]
        limit: Option<u32>,
    },

    /// Search feed entries
    #[command(name = "search")]
    Search {
        /// Feed URL
        #[arg(long, short)]
        url: String,
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Number of entries
        #[arg(long, short)]
        limit: Option<u32>,
    },

    /// Discover feeds on a webpage
    #[command(name = "discover")]
    Discover {
        /// Webpage URL
        #[arg(long, short)]
        url: String,
    },
}

/// bioRxiv tools
#[derive(Subcommand, Clone)]
pub enum BiorxivTools {
    /// Get recent preprints
    #[command(name = "recent")]
    Recent {
        /// Server (biorxiv or medrxiv)
        #[arg(long, short)]
        server: String,
        /// Number of papers (max 100)
        #[arg(long, short)]
        count: Option<u32>,
    },

    /// Get preprints by date range
    #[command(name = "date-range")]
    DateRange {
        /// Server (biorxiv or medrxiv)
        #[arg(long, short)]
        server: String,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start_date: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end_date: String,
    },

    /// Get preprint by DOI
    #[command(name = "paper", alias = "get-paper")]
    Paper {
        /// Server (biorxiv or medrxiv)
        #[arg(long, short)]
        server: String,
        /// DOI
        #[arg(long, short)]
        doi: String,
    },
}

/// Open-access paper lookup tools (via OpenAlex/Unpaywall)
///
/// Find freely available PDF versions of academic papers.
/// Uses OpenAlex by default, with optional Unpaywall support for better coverage.
#[derive(Subcommand, Clone)]
pub enum ScihubTools {
    /// Lookup open-access PDF and metadata by DOI
    ///
    /// Queries OpenAlex (and optionally Unpaywall) to find freely available
    /// versions of the paper. Returns PDF URL, title, authors, year, and
    /// whether an open-access copy was found.
    ///
    /// Common DOI formats:
    ///   10.1038/nature12373           (standard DOI)
    ///   10.48550/arXiv.1706.03762     (arXiv DOI)
    ///   10.1371/journal.pone.0000308  (PLOS DOI)
    #[command(name = "paper", alias = "get-paper")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools scihub paper --doi 10.1038/nature12373
  rzn-tools scihub paper --doi 10.48550/arXiv.1706.03762
  rzn-tools scihub paper -d 10.1371/journal.pone.0000308")]
    Paper {
        /// The DOI (Digital Object Identifier) of the paper
        ///
        /// Examples: 10.1038/nature12373, 10.48550/arXiv.1706.03762
        #[arg(long, short)]
        doi: String,
    },

    /// Search for papers by title, author, or keywords via OpenAlex
    ///
    /// Returns matching papers with metadata and open-access PDF links
    /// when available.
    #[command(name = "search")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools scihub search --query \"attention mechanism\"
  rzn-tools scihub search --query \"CRISPR\" --oa-only --limit 10
  rzn-tools scihub search --query \"Jane Smith\" --page 2")]
    Search {
        /// Search query (title, author, keywords)
        #[arg(long, short)]
        query: String,
        /// Maximum number of results (default 10, max 200)
        #[arg(long, short, default_value_t = 10)]
        limit: u32,
        /// Page number for pagination
        #[arg(long, short, default_value_t = 1)]
        page: u32,
        /// Only return open-access works
        #[arg(long)]
        oa_only: bool,
    },

    /// Look up multiple DOIs concurrently
    ///
    /// Accepts a comma-separated list of DOIs and resolves them in parallel.
    /// Failed lookups return success=false instead of aborting the batch.
    #[command(name = "batch")]
    #[command(after_help = "\x1b[1;33mExamples:\x1b[0m
  rzn-tools scihub batch --dois \"10.1038/nature12373,10.1371/journal.pone.0000308\"
  rzn-tools scihub batch --dois \"10.48550/arXiv.1706.03762,10.1016/j.cell.2023.01.001\"")]
    Batch {
        /// Comma-separated list of DOIs to look up (max 50)
        #[arg(long, short)]
        dois: String,
    },
}

/// macOS tools
#[derive(Subcommand, Clone)]
pub enum MacosTools {
    /// Run AppleScript or JXA
    #[command(name = "script", alias = "run-script")]
    Script {
        /// Script language (applescript, javascript, jxa)
        #[arg(long, short, default_value = "applescript")]
        language: String,
        /// Script source code
        #[arg(long, short)]
        script: String,
        /// Optional parameters (JSON)
        #[arg(long)]
        params: Option<String>,
        /// Max output characters
        #[arg(long)]
        max_output_chars: Option<u32>,
    },

    /// Show notification
    #[command(name = "notify", alias = "notification")]
    Notify {
        /// Title
        #[arg(long, short)]
        title: Option<String>,
        /// Message
        #[arg(long, short)]
        message: String,
        /// Subtitle
        #[arg(long)]
        subtitle: Option<String>,
    },

    /// Reveal file in Finder
    #[command(name = "reveal")]
    Reveal {
        /// File path
        #[arg(long, short)]
        path: String,
    },

    /// Get clipboard content
    #[command(name = "clipboard", alias = "get-clipboard")]
    GetClipboard,

    /// Set clipboard content
    #[command(name = "set-clipboard")]
    SetClipboard {
        /// Text to copy
        #[arg(long, short)]
        text: String,
    },

    /// Run Apple Shortcut
    #[command(name = "shortcut", alias = "run-shortcut")]
    Shortcut {
        /// Shortcut name
        #[arg(long, short)]
        name: String,
        /// Optional input (JSON)
        #[arg(long, short)]
        input: Option<String>,
    },
}

/// Apple Messages tools
#[derive(Subcommand, Clone)]
pub enum AppleMessagesTools {
    /// List recent chats using privacy-safe aliases
    #[command(name = "chats", alias = "list-chats")]
    Chats {
        /// Maximum chats to return
        #[arg(long, short, default_value_t = 20)]
        limit: u32,
    },

    /// Read recent messages
    #[command(name = "messages", alias = "get-messages")]
    Messages {
        /// Privacy-safe alias from list-chats/list-aliases
        #[arg(long)]
        alias: Option<String>,
        /// Deprecated raw chat identifier fallback
        #[arg(long)]
        chat_identifier: Option<String>,
        /// Only include messages on/after this timestamp
        #[arg(long)]
        since: Option<String>,
        /// Only include messages with rowid greater than this
        #[arg(long)]
        since_message_id: Option<i64>,
        /// Maximum messages to return
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },

    /// Send a message
    #[command(name = "send", alias = "send-message")]
    Send {
        /// Privacy-safe alias from list-chats/list-aliases
        #[arg(long)]
        alias: Option<String>,
        /// Deprecated raw phone number or iMessage email fallback
        #[arg(long)]
        recipient: Option<String>,
        /// Message text
        #[arg(long, short)]
        message: String,
    },

    /// List saved aliases
    #[command(name = "aliases", alias = "list-aliases")]
    Aliases,

    /// Create or update an alias
    #[command(name = "set-alias", alias = "upsert-alias")]
    SetAlias {
        /// Alias name
        #[arg(long)]
        alias: String,
        /// Raw phone number or iMessage email
        #[arg(long)]
        identifier: String,
    },

    /// Remove an alias
    #[command(name = "remove-alias", alias = "rm-alias")]
    RemoveAlias {
        /// Alias name
        #[arg(long)]
        alias: String,
    },
}

/// Spotlight tools
#[derive(Subcommand, Clone)]
pub enum SpotlightTools {
    /// Full-text content search
    #[command(name = "search", alias = "search-content")]
    SearchContent {
        /// Search query
        #[arg(long, short)]
        query: String,
        /// Directory to search in
        #[arg(long, short)]
        directory: Option<String>,
        /// File kind filter
        #[arg(long, short)]
        kind: Option<String>,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },

    /// Search by file name
    #[command(name = "name", alias = "search-by-name")]
    SearchByName {
        /// File name
        #[arg(long, short)]
        name: String,
        /// Directory to search in
        #[arg(long, short)]
        directory: Option<String>,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },

    /// Search by file kind
    #[command(name = "kind", alias = "search-by-kind")]
    SearchByKind {
        /// File kind (pdf, image, video, etc.)
        #[arg(long, short)]
        kind: String,
        /// Directory to search in
        #[arg(long, short)]
        directory: Option<String>,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },

    /// Search recent files
    #[command(name = "recent", alias = "search-recent")]
    SearchRecent {
        /// Number of days
        #[arg(long, short, default_value_t = 7)]
        days: u32,
        /// File kind filter
        #[arg(long, short)]
        kind: Option<String>,
        /// Directory to search in
        #[arg(long)]
        directory: Option<String>,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },

    /// Get file metadata
    #[command(name = "metadata", alias = "get-metadata")]
    Metadata {
        /// File path
        #[arg(long, short)]
        path: String,
    },

    /// Raw Spotlight query
    #[command(name = "raw", alias = "raw-query")]
    RawQuery {
        /// Raw mdfind query
        #[arg(long, short)]
        query: String,
        /// Directory to search in
        #[arg(long, short)]
        directory: Option<String>,
        /// Maximum results
        #[arg(long, short, default_value_t = 50)]
        limit: u32,
    },
}
