---
workflow_version: 1
tracker_schema_version: 7
tracker:
    kind: tusker_vault
    dispatch_states:
        - ready
        - rework
    review_states:
        - review
    terminal_states:
        - done
        - cancelled
        - superseded
agents:
    default: codex_exec
    enabled:
        - codex_exec
        - claude-code
    max_concurrent_agents: 2
    max_concurrent_agents_by_state:
        rework: 1
runtime:
    poll_interval_ms: 5000
    lease_ttl_ms: 900000
    max_active_runs_per_project: 1
    max_continuation_retries: 3
    budget:
        enabled: true
        per_attempt_input_tokens: 50000000
        per_attempt_output_tokens: 500000
        per_task_input_tokens: 250000000
        per_task_output_tokens: 2500000
        daily_input_tokens: 10000000000
        daily_output_tokens: 100000000
    serve:
        enabled: true
        addr: 127.0.0.1:7420
    sentinel:
        checks:
            - held_lease_dispatch_eligible
            - attempt_count_within_caps
            - fresh_heartbeat_pid_live
            - unique_active_lease_per_task
            - active_spend_monotonic
            - last_poll_advanced
        fresh_heartbeat_ms: 120000
workspace:
    root: .
    strategy: in_place
retry:
    max_attempts: 3
    backoff_ms:
        - 30000
        - 120000
        - 600000
reviewer:
    enabled: true
    runner: codex_exec
    actor: agent:reviewer/codex
    auto_close_risks:
        - low
        - medium
    human_required_risks:
        - high
        - critical
    prompt: |-
        You are the independent Tusker reviewer for {{ note.id }}.

        Review only. Do not edit implementation files. If the work needs changes, mark the task `rework` with a specific acceptance/proof reason instead of fixing it yourself.

        Task:
        - ID: {{ note.id }}
        - Title: {{ note.title }}
        - Risk: {{ note.risk }}
        - Status: {{ note.status }}
        - Attempt: {{ attempt.id }}
        - Workspace: {{ workspace.path }}
        - Vault: {{ vault.path }}

        Policy:
        - Reviewer actor: {{ reviewer.actor }}
        - Auto-close allowed: {{ reviewer.auto_close_allowed }}
        - Human close required: {{ reviewer.human_required }}

        Checklist:
        1. Read the task acceptance contract, proof mode, verification rows, evidence cards, and gates.
        2. Inspect the current diff against the task scope. Call out surprise files or drive-by refactors.
        3. Run the smallest verification commands needed to prove the acceptance contract.
        4. Confirm project skill/domain canon changes only when the task changed durable project knowledge.
        5. For high or critical risk, leave the task in review with a human-actionable recommendation.
        6. If a caveat changes scope, decide whether it is acceptable or requires rework.

        If the task fails review, run:
        tusker status {{ note.id }} rework --by {{ reviewer.actor }} --reason "<specific unmet acceptance item>"

        If auto-close is allowed and every check passes, run:
        {{ reviewer.verify_command }}
        {{ reviewer.close_command }}

        If human close is required and every check passes, do not run `verify` or `close`. Leave the task in `review` and state the human-review recommendation in your final response.
external_loop:
    maxcycles: 3
    maxrepaircontinuations: 2
    maxexternalthreads: 5
    wallclocktimeouthours: 8
runners:
    claude-code:
        kind: claude-code
        command: claude -p --output-format stream-json --input-format stream-json --permission-mode bypassPermissions
    codex_exec:
        kind: codex_exec
        command: codex exec --json --skip-git-repo-check -
codex:
    command: codex exec --json --skip-git-repo-check -
    approval_policy: on-request
    thread_sandbox: workspace-write
    turn_sandbox_policy: workspace-write
    turn_timeout_ms: 600000
    read_timeout_ms: 30000
    stall_timeout_ms: 120000
    max_turns: 1
codex_cloud:
    command: ""
    status_command: ""
    collect_command: ""
    environment_id: ""
    apply_mode: ""
    pr_mode: ""
claude:
    command: claude -p --output-format stream-json --input-format stream-json --permission-mode bypassPermissions
extensions:
    enabled: false
    allowed_tools: []
    allowed_mcps: []
    allow_tusker_read_tools: false
hooks:
    after_workspace_create: []
    before_workspace_remove: []
fanout:
    enabled: false
    max_children: 0
    allowed_child_types: []
    merge_rule: manual_review
---

## Routing

You are working on {{ note.id }} for {{ project.name }}. Dispatch only makes sense when this task is in a dispatch state (`ready` or `rework`) and the workspace is ready at {{ workspace.path }}.

## Hard stop check

Before doing work, run `tusker closeout status {{ note.id }} --json` when the V7 closeout command is available. If it reports `agent_action=stop_until_human_response`, do not validate, inspect files, spawn subagents, or modify Tusker records. Reply with the pending human gates/proof and whether the closeout checkpoint or review packet is still needed.

Revalidate only after you edited files, a task/gate/evidence state changed, the closeout fingerprint no longer matches, or the user explicitly asked for fresh validation.

## Prompt

Use the installed Tusker skill bundle for durable task semantics and proof discipline. Work inside {{ workspace.path }}. Treat {{ repo.root }} as the source repository root for context only unless the task explicitly requires comparing against it.

Item: {{ note.title }}
Record: {{ note.record_id }}
Type: {{ note.type }}
Attempt: {{ attempt.number }}
Workflow: {{ workflow.path }}
Vault: {{ vault.path }}

## Command budget

Use the smallest command that proves or locates the next fact. Prefer packets/capsules, path-scoped status/search, repo-configured wrappers and build-lock/status commands, and redirected logs with small tails. Report validation as command + PASS/FAIL plus the first actionable failure; do not paste raw transcripts or repeat unchanged-state updates.

## Worker protocol

Each dispatched attempt starts with fresh runner context. Use the injected task packet, `.tusker/scratch/<TASK-ID>/PLAN.md`, and previous structured outcome as the handoff; do not query or replay predecessor transcripts. Work one task only. Search before implementing, do not add placeholders or stubs, and run the configured backpressure commands serially.

## Merge lane guard

Do not push or merge directly to the default branch/main. Finish the task proof, then use `tusker land {{ note.id }}`; the serialized landing lane is the only authorized path from task branches into integration branches and main.

## External Apply Inputs

Some tasks may have external apply inputs collected by Tusker under `architect/{{ note.id }}/` or a workspace-local mirror of that directory.

When that directory contains exactly one `*.patch` or `*.diff` file:

1. inspect the task acceptance and verification contract first;
2. run `git apply --check --3way <patch>`;
3. apply with `git apply --3way <patch>` only after the check passes;
4. resolve conflicts only when the resolution is mechanical and clearly within the task contract;
5. run the task verification commands;
6. record compact verification evidence;
7. use `tusker finish {{ note.id }} --request-review` when machine proof is complete;
8. create a concrete gate or move to rework/blocked when proof cannot be completed.

If there are zero patches, multiple patches, a patch outside scope, or an ambiguous conflict, stop and report the blocker through Tusker. Do not invent or silently repair patches.

## Completion contract

Satisfy the task proof mode. For proof_mode=inline, record concise verification rows with `tusker verify add`; do not create evidence files. For card/artifact/audit, create only the evidence the proof mode requires. When machine work is complete and only human-owned proof or gates remain, run `tusker closeout <task-id> --emit-packet --validate "<command>"`, then stop. When the work is demonstrably ready for verification, use `tusker finish <task-id> --request-review` so the task reaches `review` or a branch-safe `propose status ... --status review` proposal is created. Attempt handoff alone is not a review request. If proof is blocked, create/propose a gate with a concrete owner, action, and verification instead of appending negative evidence.

## Reviewer contract

If `reviewer.enabled` is true, tasks in `review` may be dispatched to `reviewer.runner` for independent review. The reviewer must not edit implementation files. Low/medium risks can be verified and closed by `reviewer.actor` after all gates pass; high/critical risks stay in `review` for human verification and close.

## Retry policy

Retry only transient infrastructure failures. Human-directed rework creates a new task revision; runtime activity remains in the run/lease store.

## Human override policy

Humans may edit tasks directly, but runtime state belongs to the daemon store.
