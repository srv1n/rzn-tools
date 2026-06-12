---
name: tool-design
description: |
  Design review for any tool we ship to agents (connectors, tool handlers,
  CLI commands surfaced to LLMs, MCP tools). Run this before merging a new
  tool or when refactoring an existing one. Encodes Anthropic's "Writing
  tools for agents" guidance (anthropic.com/engineering/writing-tools-for-agents)
  as a concrete, rzn-tools-flavored checklist.

  Use when: adding a new connector tool, adding a handler to an existing
  connector, writing MCP tool definitions, refactoring tool signatures,
  reviewing a PR that touches tool surface area, or when an agent's
  transcripts show it misusing a tool.
allowed-tools:
  - Read
  - Write
  - Edit
  - Grep
  - Glob
  - Bash
  - WebFetch
---

# Tool Design Review

You are reviewing a tool that an LLM agent will call. The tool is a contract
between a deterministic system (our code) and a non-deterministic caller
(an LLM). Optimize for the caller's success, not for API completeness or
developer ergonomics.

Source of truth: https://www.anthropic.com/engineering/writing-tools-for-agents

## When to run this skill

Invoke this review any time the tool surface changes:

- New connector handler / action (`rzn_tools_core/src/connectors/*/handlers/*.rs`)
- New tool registered in a tool registry or MCP definition
- Signature or description change on an existing tool
- PR review for tool-shaped code
- Before a release that adds agent-facing capability

If the user asks you to build a new tool from scratch, run this review *as
you design it*, not after.

## Workflow

1. **Identify the tool(s) in scope.** Ask the user or read the diff. List
   name, parameters, return shape, and the one-sentence description an
   agent sees.
2. **Score against each section below.** For every section, write either
   PASS with a sentence of evidence, or FAIL with the specific problem and
   a concrete fix.
3. **Rank fixes.** Group findings into Must-fix (agent will misuse the
   tool), Should-fix (wastes tokens or adds friction), and Nice-to-have.
4. **Propose the revised tool.** Produce a concrete before/after — the
   updated signature, description, and sample response — not just advice.
5. **If requested, apply the fixes.** Edit the relevant files. Do not
   edit without explicit go-ahead when the change touches public contracts.

Keep the review tight. A good review is a short list of real problems with
fixes, not a generic lecture.

## Review checklist

### 1. Is this tool worth existing?

More tools ≠ better agents. Extra tools crowd the context and dilute tool
selection.

- Does it map to something an agent actually *wants to do*, or is it a
  thin wrapper over one REST endpoint? Prefer the former.
- Could it be absorbed into a sibling tool? e.g. `list_x` +
  `get_x_by_id` often collapses into `search_x` that returns enough
  fields to skip the follow-up call.
- Does it overlap with an existing tool? If so, the agent will pick
  randomly. Merge, rename, or delete.

Good: `search_contacts`, `schedule_event`, `get_customer_context`.
Bad: `list_users` + `list_events` + `create_event` as three separate tools.

### 2. Name and namespace

- Prefix related tools consistently: `gmail_search`, `gmail_send`,
  `gmail_thread_get`. This offloads "which service?" from agent
  reasoning to pattern matching.
- For larger surfaces use two-level prefixes: `asana_projects_search`,
  `asana_users_search`.
- Name verbs match intent: `search_` for retrieval, `get_` for a
  specific known ID, `create_/update_/delete_` for writes. Do not use
  `fetch_` and `get_` interchangeably across the codebase.
- Parameter names are specific: `user_id`, not `user`; `thread_id`, not
  `id`; `query_text`, not `q`.

### 3. Description (what the agent actually reads)

Write it as if onboarding a new teammate who has never seen this system.

- Lead sentence: what the tool does, in plain language, no jargon.
- Define any term the agent couldn't guess (what is a "thread"? a "run"?
  a "capability"?).
- State the relationship to sibling tools: "Use `x_search` first to find
  an id, then pass it to this tool."
- Call out non-obvious constraints: rate limits, required auth scopes,
  destructive side effects, idempotency.
- Include one concrete example input and the shape of the response.
- For MCP tools, set annotations honestly: `readOnlyHint`,
  `destructiveHint`, `openWorldHint`.

Parameter descriptions matter as much as the tool description — each
parameter should say what valid values look like, not just its type.

### 4. Response shape: high signal, low tokens

The response is what the agent spends context on. Every field should earn
its place.

- Prefer semantic fields over technical ones: `name`, `image_url`,
  `file_type` over `uuid`, `256px_image_url`, `mime_type`.
- Only expose IDs the agent will need for a downstream call. If no
  other tool consumes the ID, drop it.
- Offer a `response_format` enum (`"concise" | "detailed"`) when a
  tool can reasonably return at two verbosity levels. Default to
  `"concise"`. A Slack-thread example in the source article dropped
  206 tokens → 72 tokens this way.
- Consistent key ordering and shape across results so the agent can
  pattern-match.

### 5. Token efficiency and truncation

- Paginate anything that can return >50 items. Sensible default page
  size (10–25). Expose `page_token` or `offset`, not raw SQL cursors.
- Cap total response tokens. Claude Code's default is ~25k; mirror that.
- When truncating, say so in-band with actionable guidance:
  `[Results truncated to 100 items. Narrow with `status=` or a more
  specific query.]`
- Support filtering at the tool level. If the agent has to fetch
  everything and scan, the tool is doing the wrong thing.

### 6. Errors are UX

Every error is a prompt. An opaque error teaches the agent nothing; a
good one corrects it in one shot.

- Name the field that was wrong.
- State the allowed values or format.
- Show a corrected example call.

Bad: `Error 400: invalid request`
Good: `Invalid parameter 'status': must be one of "pending",
"completed", "cancelled". Example: search_tasks(user_id=123,
status="pending")`

Error shape should be stable across tools in the same namespace so the
agent can learn it once.

### 7. Side effects and safety

- Any destructive or externally visible action (sending mail, creating
  calendar events, posting to Slack) must say so in the description
  and — for MCP — set `destructiveHint: true`.
- Writes should be idempotent where possible (accept an
  `idempotency_key`) so retries don't double-post.
- Reads of personal data must respect the repo's privacy rules
  (`CLAUDE.md`): do not have the tool auto-execute in ways that read
  the user's mail/messages/notes without explicit permission.

### 8. Response format (XML / JSON / Markdown)

No universal winner. Pick based on eval results, not taste. Defaults
that usually work in this repo:

- Structured data the agent will parse → JSON.
- Prose or mixed content the agent will quote back → Markdown.
- Long tabular data → Markdown tables beat JSON arrays of objects for
  token cost when the agent only needs to read, not manipulate.

## Anti-patterns (hard fails)

| Anti-pattern | Why it fails | Fix |
|---|---|---|
| Wraps one REST endpoint 1:1 | Agent has to orchestrate many calls | Consolidate around the *task* |
| Returns UUIDs as the primary identifier agents see | Hallucination magnet | Return names; keep IDs only if a downstream tool needs them |
| `list_*` that returns everything | Brute-force scan burns context | `search_*` with filters + pagination |
| No pagination / no truncation notice | Blows past context limits silently | Paginate; say when you truncated and how to narrow |
| Error = HTTP status only | Agent can't self-correct | Actionable message with example |
| Two tools do overlapping things | Non-deterministic tool choice | Merge or differentiate clearly |
| Generic param names (`id`, `data`, `input`) | Ambiguous at call time | Specific: `thread_id`, `message_body`, `query_text` |
| Description assumes insider knowledge | Agent misuses it | Write for a new hire; define terms |
| Write tool with no idempotency key | Retries cause duplicates | Accept `idempotency_key` |

## Evaluation (do this before shipping a tool)

A tool isn't designed — it's *measured*.

1. **Write 5–10 realistic tasks** that require this tool. Favor
   multi-step, grounded-in-real-data prompts over toy ones.

   Good: "Customer ID 9182 was charged three times this week; find all
   relevant logs and determine if other customers were affected."

   Weak: "Search logs for customer_id=9182."

2. **Run them in an agent loop.** Direct API calls with a simple
   while-loop of (LLM call → tool execution). One loop per task.
   Instruct the agent to emit reasoning before tool calls (or enable
   interleaved thinking).

3. **Collect metrics per task:**
   - Task success (exact match or LLM-judge against ground truth)
   - Tool-call count
   - Tool errors
   - Total tokens
   - Wall-clock time

4. **Read the transcripts.** This is where the insight lives. Look for:
   - Redundant calls (tool returned too little; agent had to call twice)
   - Oscillation between two tools (naming/overlap problem)
   - Parameter-shape confusion (description problem)
   - Agent inventing fields that don't exist (hallucination → tighten
     response shape, use semantic names)

5. **Iterate.** Often one description tweak is worth more than a
   refactor. The SWE-bench Verified jump in the source article came
   from description edits, not new tools.

6. **Hold out a test set** so you don't overfit descriptions to the
   tasks you're measuring against.

## Output format for this skill

When asked to review a tool, produce:

```
## Tool: <name>

### Summary
<one paragraph: what it does, what's working, what's not>

### Findings
- [MUST] <problem> — <fix>
- [SHOULD] <problem> — <fix>
- [NICE] <problem> — <fix>

### Proposed revision
<before / after of signature + description + sample response>

### Suggested eval tasks
1. <task>
2. <task>
...
```

If you're *designing* a tool from scratch rather than reviewing one,
produce the revision section directly, with rationale tied to the
checklist above.

## rzn-tools specifics

- Connectors live in `rzn_tools_core/src/connectors/<name>/`. Each
  handler exposed to agents goes through the tool mapping layer —
  check `rzn_tools_core/src/tool_mappings/` or the connector's
  `list_tools` / `call_tool` impl when reviewing surface area.
- Tool registrations usually appear in the connector's
  `impl Connector::list_tools` — that's the authoritative surface
  the agent sees, not the internal handler functions.
- Privacy rules in `/CLAUDE.md` override anything here for personal-
  data connectors (Mail, Notes, Messages, Reminders, Contacts, IMAP).
  A tool that reads personal data should not be invoked by the agent
  during this review — describe test commands and hand them back.
- Release builds ship with `full` features. If you're adding a tool
  behind a feature flag, verify it's included in the release feature
  set or it will look "missing" in shipped binaries.
