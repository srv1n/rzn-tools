---
schema: "tusker.epic/v7"
kind: "epic"
id: "ADS"
project: "rzn-tools"
title: "Ads & campaign intelligence connectors"
status: "ready"
owner: "human:sarav"
priority: "p2"
domains: []
next_task_number: 1
next_gate_number: 1
next_decision_number: 1
created_at: "2026-07-10T04:06:41Z"
updated_at: "2026-07-10T04:19:59Z"
state_rev: "sha256:0838a5eeaf14cdc9a13a4990decc0ac451213dad6e50318f901f589a30d59d04"
---

# ADS · Ads & campaign intelligence connectors

## Thesis

API connectors and analysis tooling for paid-ads reporting and competitor ad-creative intelligence, feeding the SMB agent-in-a-box offering.

## Success criteria

- [ ] Define success criteria.

## Current decision

TBD.

## Open gates

<!-- tusker:generated open-gates -->

| Gate | Owner | Blocks | Action |
|---|---|---|---|
| [[ADS-G-0001]] | human:sarav | [[ADS-T-0002]] | Create a Meta developer app, attach an ad account, and provision a long-lived system-user access token with ads_read; place it in .env as META_ADS_ACCESS_TOKEN (+ account id). |
| [[ADS-G-0002]] | human:sarav | [[ADS-T-0003]] | Apply for a Google Ads developer token (Basic access), create an OAuth client, and mint a refresh token for the test/MCC account; place GOOGLE_ADS_DEVELOPER_TOKEN, GOOGLE_ADS_OAUTH_CLIENT_ID, GOOGLE_ADS_OAUTH_CLIENT_SECRET, GOOGLE_ADS_REFRESH_TOKEN in .env. |

## Active work

<!-- tusker:generated active-work -->

| Task | Status | Next owner | Next action |
|---|---|---|---|
| [[ADS-T-0001]] | ready | agent | Execute the task contract and satisfy proof mode. |
| [[ADS-T-0002]] | ready | human:sarav | Accept, waive, or return rework for ADS-G-0001. |
| [[ADS-T-0003]] | ready | human:sarav | Accept, waive, or return rework for ADS-G-0002. |
| [[ADS-T-0004]] | backlog | blocked_dependency | Wait for dependency ADS-T-0001 to reach review with satisfied proof or done. |

## Recently completed

<!-- tusker:generated recently-completed -->

| Task | Accepted by | Closed at |
|---|---|---|
| _None._ |  | |
