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
    #[command(alias = "ls")]
    List,
    #[command(alias = "init")]
    Setup {
        connector: Option<String>,
    },
    #[cfg(feature = "serve")]
    Configure {
        #[command(subcommand)]
        target: ConfigureTarget,
    },
    #[cfg(feature = "serve")]
    Serve {
        bind: Option<String>,
        #[arg(long, value_delimiter = ',')]
        allow_hosts: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        connectors: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        add_connectors: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        remove_connectors: Vec<String>,
        #[arg(long)]
        all_connectors: bool,
        #[arg(long)]
        list_connectors: bool,
        #[arg(long)]
        local_only: bool,
    },
    Search {
        connector_or_query: String,
        query: Option<String>,
        #[arg(short, long, default_value_t = 10)]
        limit: u32,
        #[arg(short, long)]
        profile: Option<String>,
        #[arg(short = 's', long = "sources")]
        connectors: Option<String>,
        #[arg(short, long, default_value = "grouped")]
        merge: String,
        #[arg(long)]
        add: Option<String>,
        #[arg(long)]
        exclude: Option<String>,
    },
    Get {
        connector: String,
        id: String,
        #[arg(long)]
        field: Option<String>,
    },
    #[command(alias = "f")]
    Fetch {
        input: String,
        #[arg(long, default_value = "raw", value_parser = ["raw", "normalized_v1", "display_v1"])]
        output_format: String,
    },
    #[command(alias = "patterns")]
    Formats,
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    Connectors,
    Tools {
        connector: Option<String>,
    },
    /// Call any connector tool. Pass an object exactly as shown by `rzn-tools tools <connector>`.
    #[command(
        after_help = "Example:\n  rzn-tools call youtube get --args '{\"video_id\":\"dQw4w9WgXcQ\",\"response_format\":\"concise\"}'"
    )]
    Call {
        connector: String,
        tool: String,
        #[arg(long, default_value = "{}", value_name = "JSON_OBJECT")]
        args: String,
    },
    Ingest {
        #[command(subcommand)]
        action: IngestAction,
    },
    Pricing {
        connector: Option<String>,
        tool: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    Usage {
        connector: Option<String>,
        tool: Option<String>,
        #[arg(long)]
        run: Option<String>,
        #[arg(long, conflicts_with = "run")]
        last: bool,
    },
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
    #[command(alias = "systems")]
    Workflows {
        #[command(subcommand)]
        action: WorkflowAction,
    },
    Skills {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// First-class YouTube video, playlist, channel, and transcript workflows.
    #[command(name = "youtube", alias = "yt")]
    Youtube {
        #[command(flatten)]
        args: YoutubeArgs,
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
#[cfg(feature = "serve")]
pub enum ConfigureTarget {
    /// Save Cloudflare tunnel defaults for `rzn-tools serve`
    Cloudflare {
        #[command(subcommand)]
        action: CloudflareConfigureAction,
    },
}

#[derive(Subcommand, Clone)]
#[cfg(feature = "serve")]
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
