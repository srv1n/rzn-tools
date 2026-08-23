---
schema: "tusker.domain/v7"
kind: "domain"
id: "project"
project: "rzn-tools"
title: "Project"
status: "current"
summary: "Durable knowledge for the rzn-tools repository."
capsule:
  what: "Domain index for Project; routes agents to canon and owned knowledge files."
  use_when: "Use when a task touches project behavior or needs the domain reading order."
  skip_when: "Skip when another domain is narrower or task proof/gates are the target."
source_of_truth:
  - "knowledge/domains/project/CANON.md"
canonical_files:
  - "INDEX.md"
  - "CANON.md"
created_at: "2026-08-23T10:54:54Z"
updated_at: "2026-08-23T10:54:54Z"
state_rev: "sha256:934e30b5ee6758b5d8b6c5057ee8cb92abad9e2f3911ada41f67afb4d93859c9"
---

# Project

## Summary

`rzn-tools` provides one CLI and MCP surface for many data sources.

## Read This When

- You need current source-of-truth context for project.
- You are changing behavior owned by this domain.

## Canonical Files

- CANON.md - current durable truth.
- INDEX.md - domain map and routing hints.

## Runbooks

- See `docs/system/operations.md` for install and run steps.
- See `docs/system/development.md` for build and test steps.

## Interfaces

- `rzn_tools_core::Connector`
- `rzn-tools` CLI
- `rzn-tools-mcp` MCP server

## Invariants

- Keep durable truth in CANON.md.
- Put procedural guidance in runbooks/.
- Keep secrets out of the repository.

## Sources

- Raw external input belongs in sources/. Do not treat root docs/ or site output as canonical V7 knowledge.

## Glossary

- See glossary.md.

## Current Work

- _No current work linked._
