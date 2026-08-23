# Contribution workflow

This workflow keeps changes small and reviewable.

## Principles

- Problems should be legible before implementation starts.
- Review should start with evidence, not archaeology.
- AI is allowed, but slop is not.
- Contributors must understand their own changes.
- Raw transcripts are optional appendix material, not required reading.

## Flow

```text
idea or report
   ↓
clear task
   ↓
review signal
   ↓
implementation
   ↓
change summary and evidence
   ↓
review
```

## Before review

Before review, explain:

- what changed
- why it changed
- how it was tested
- what the risk is
- what a reviewer should focus on

If an assistant helped, say so. The author must understand the final behavior.

## Maintainer checks

- Keep feature and architecture work out of surprise changes.
- Reject refactor-only churn unless it is requested.
- Ask for evidence for user-visible changes.
- Match the check to the risk.
