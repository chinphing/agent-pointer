# Taskboard Lifecycle And Fields

This document defines the current taskboard conventions.

## Field semantics

- `details`: execution plan, implementation details, and key points.
- `progress`: partial progress for combinational or in-flight work.
- `validate`: final goal validation only.

Guideline:

- Put process detail in `details`.
- Put incremental checkpoint state in `progress`.
- Put acceptance evidence in `validate`.

## Details writing format

Use a concise markdown table in `details` when work is multi-step:

| step | action | key_points | risk | done_when |
| --- | --- | --- | --- | --- |
| 1 | ... | ... | ... | ... |

Keep lines short.
Avoid long prose paragraphs.

## Injection order

Runtime injection order for current task content is:

1. `details`
2. `progress`
3. `validate`

## Lifecycle binding rules

- A taskboard is bound to a user message.
- One conversation can contain multiple parent taskboards.
- An ended taskboard must not be injected into later user turns.

Continuation rule:

- If the newest user message clearly expresses continuation intent,
  reuse the latest unfinished taskboard.
- Otherwise create a new taskboard bound to that user message.

## UI behavior rules

- Ended taskboards should render as normal inline panels.
- Ended taskboards should not use sticky scroll behavior.
- Unfinished taskboards may stay sticky to support active execution.
