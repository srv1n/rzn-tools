# Local filesystem

Connector name: `localfs`

This connector reads files and directories directly. It has no index, watcher,
root allowlist, or path sandbox.

| Tool | Use |
| --- | --- |
| `list_files` | List one directory, with optional recursion and extension filters. |
| `get_file_info` | Read file metadata. |
| `extract_text` | Extract text from supported documents. |
| `get_structure` | Read document structure. |
| `get_section` | Read one document section. |
| `search_content` | Search text in a path. |

Supported extraction includes PDF, EPUB, DOCX, HTML, Markdown, source code, and
plain text. Results are bounded. An unreadable path returns an error.

Security: callers can supply paths. Run the process with only the filesystem
permissions it needs. Do not expose this connector to untrusted MCP users.

Code: `rzn_tools_core/src/connectors/localfs/mod.rs`

