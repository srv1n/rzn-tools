# AI contribution policy

AI-assisted work is allowed.

What is not allowed is submitting code that the human contributor does not understand.

## Required when AI is used

- disclose that AI was used
- disclose the tool or tools used
- summarize the degree of assistance
- verify the final behavior manually when appropriate
- resolve bot or review comments intentionally, not mechanically

## Not required

- full transcripts for every change
- giant prompt dumps
- performative process theater

## Strong preference

Use structured summaries instead of transcript walls.

For non-trivial AI-assisted changes, prefer a Tusker explainer packet over raw transcripts:

```bash
tusker packet <TASK-ID> --for explainer --write
```

It should help the human explain the change in their own words; it is not proof by itself.
