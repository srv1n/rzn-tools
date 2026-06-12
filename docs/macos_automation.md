# macOS Automation MCP Server

## Overview
- Provides macOS automation helpers (AppleScript, JXA, Shortcuts, clipboard, notifications).
- Tools are exposed through the macOS connector; MCP clients will see names prefixed as `macos/<tool>`.
- Execution prefers the in-process `osakit` runtime when compiled with `--features macos-automation`, falling back to `/usr/bin/osascript`.

## Build & Run
- Enable the feature flag when compiling the MCP server:
  - `cargo build -p rzn_tools_mcp --features macos-automation`
- Launch the server over stdio (debug build shown):
  - `cargo run -p rzn_tools_mcp --features macos-automation`
- For direct binaries, start `target/debug/mcp_server` (or `target/release/mcp_server`) and keep stdin/stdout wired to the MCP client.

## JSON-RPC Handshake
- Initialize:
  - `{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"0.1.0","capabilities":{},"clientInfo":{"name":"desktop-app","version":"1.0"}},"id":1}`
- List tools (shows `macos/run_script`, `macos/show_notification`, etc.):
  - `{"jsonrpc":"2.0","method":"tools/list","params":{},"id":2}`
- Invoke a tool:
  - `{"jsonrpc":"2.0","method":"tools/call","params":{"name":"macos/run_script","arguments":{"language":"applescript","script":"display notification \\"Hi\\""}},"id":3}`

## Tool Catalog
- `macos/run_script` — Run AppleScript or JXA.
  - Args: `{ language?: "applescript"|"javascript"|"jxa", script: string, params?: any, max_output_chars?: number }`
  - For JXA, `params` is injected as `var $params = <json>` before the script body.
  - Returns: `{ language, stdout, stderr, exit_code, truncated_stdout, truncated_stderr }`
- `macos/show_notification` — Show a macOS user notification.
  - Args: `{ message: string, title?: string, subtitle?: string }`
  - Returns: `{ ok, exit_code, stdout, stderr }`
- `macos/reveal_in_finder` — Reveal a POSIX path in Finder and bring Finder to the front.
  - Args: `{ path: string }`
  - Returns: `{ ok, exit_code, stderr }`
- `macos/get_clipboard` — Read the pasteboard as UTF-8 text.
  - Returns: `{ text }`
- `macos/set_clipboard` — Write UTF-8 text to the pasteboard.
  - Args: `{ text: string }`
- `macos/run_shortcut` — Execute an Apple Shortcut by name via the `shortcuts` CLI.
  - Args: `{ name: string, input?: any }` (non-string inputs are JSON-stringified before piping to stdin).
  - Returns: `{ ok, stdout, stderr, exit_code }`

## First-Run Permissions
- macOS may prompt for Automation and/or Accessibility access the first time scripts target other apps.
- Surface clear UX guidance in the host app so the user can approve under **System Settings → Privacy & Security**.
- Managed devices can pre-approve via MDM PPPC profiles.

## Testing Checklist
- `tools/list` shows all `macos/*` entries.
- `macos/show_notification` produces a user-visible toast.
- `macos/reveal_in_finder` focuses Finder with the requested path selected.
- `macos/set_clipboard` followed by `macos/get_clipboard` round-trips the provided text.
- `macos/run_script` (JXA) echoes `{ language: "javascript", stdout: "{\"path\":\"/Applications\"}", ... }` when run with params example.
- `macos/run_shortcut` succeeds for a locally installed Shortcut (e.g., `"Resize Image"`).

## Troubleshooting
- Non-zero `exit_code` indicates script/CLI failure; inspect `stderr`.
- Permission errors typically surface the first time AppleScript or Shortcuts are invoked.
- Use `max_output_chars` to trim verbose stdout/stderr streams; `truncated_*` flags indicate truncation.
