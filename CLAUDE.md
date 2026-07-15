# Claude Guidelines for rzn-tools

## IMPORTANT: Privacy Rules

**NEVER access the user's personal data without explicit permission.** This includes:
- Email (IMAP connector) - do NOT run fetch_messages, get_message, or any email-reading commands
- Apple Mail - do NOT run list_messages, get_message, search_mail, or any mail-reading commands
- Apple Notes - do NOT run list_notes, get_note, search_notes, or any note-reading commands
- Apple Messages - do NOT run list_chats, get_recent_messages, or any message-reading commands
- Apple Reminders - do NOT run list_reminders, get_reminder, search_reminders, or any reminder-reading commands
- Apple Contacts - do NOT run list_contacts, get_contact, search_contacts, or any contact-reading commands
- Any other connector that accesses personal/private data

When testing connectors that access personal data:
1. Build the code
2. Provide the user with test commands
3. Let the user run the commands themselves
4. User will report back any errors

## Build Commands

Every Rust compilation, including in a worktree, must use a `make` target. The
targets require `sccache`, whose user-level cache is shared between worktrees.
Install it once if needed: `cargo install sccache --locked`.

```bash
# Build with common features
make build-release CARGO_ARGS='--package rzn_tools_cli --features "exa-search,imap"'

# Build with all connectors
make build-release CARGO_ARGS='--package rzn_tools_cli --features full'

# Release builds: ALWAYS ship with all features enabled.
# (Connectors are compile-time feature-gated; releases built without `full` will appear "missing".)

# Build with Apple ecosystem (macOS only)
make build-release CARGO_ARGS='--package rzn_tools_cli --features apple-ecosystem'
```

<!-- tusker:epic-index:begin -->
## Tusker

Use Tusker for tracked repo work.

- Task mechanics live in the installed `tusker` skill.
- Project knowledge starts at `.tusker/SKILL.md`.
- Start runnable work with `tusker next`; inspect named work with `tusker show <TASK-ID> --capsule`.
- Do not read `.tusker/events`, `_generated`, `attempts`, `evidence`, `Attachments`, raw logs, or full task files unless the task explicitly requires it.
- Keep proof compact: use capsules, path-scoped status/search, and command + PASS/FAIL summaries; put noisy logs in `.tusker/scratch/<TASK-ID>/`.
- Record concise Tusker/product friction with `tusker feedback add`; skip routine progress reports.
<!-- tusker:epic-index:end -->
